// The editor island. Bundled as an IIFE exposing `window.NeedleEditor`; the Rust side binds to
// it in crates/desktop/src/editor.rs. ProseMirror owns the text while typing, Rust owns files.

import "./style.css";
import type { Node } from "prosemirror-model";
import { history } from "prosemirror-history";
import { keymap } from "prosemirror-keymap";
import { EditorState, type Plugin, Selection, TextSelection } from "prosemirror-state";
import { EditorView } from "prosemirror-view";
import { type Passage, cutPassage, restorePassage } from "./bin.ts";
import { type Typography, buildInputRules, defaultTypography } from "./inputrules.ts";
import { buildKeymaps } from "./keymap.ts";
import { type HighlightMeta, highlightKey, highlightPlugin } from "./highlight.ts";
import { type LinkMeta, type LinkResolver, linkSuggestPlugin, linksKey, linksPlugin } from "./links.ts";
import { countWords, parseMarkdown, serializeMarkdown } from "./markdown.ts";
import { openSelectionMenu, openSpellMenu } from "./menu.ts";
import {
  type CutCallbacks,
  SEAM_MS,
  type SeamMeta,
  blockCount,
  cutLineKey,
  cutLinePlugin,
  seamKey,
  seamPlugin,
  seamPosition,
} from "./pattern.ts";
import { type SpellMeta, type Spellchecker, spellcheckKey, spellcheckPlugin } from "./spellcheck.ts";

export type { LinkResolver, Passage, Spellchecker, Typography };
export { hasSpot } from "./bin.ts";

export interface EditorOptions {
  typography?: Partial<Typography>;
}

export interface MountOptions extends EditorOptions, Partial<CutCallbacks> {
  /** Called with the document's Markdown once typing pauses for `debounceMs`. */
  onChange?: (markdown: string, words: number) => void;
  /** Called with a passage just cut to the bin, which the app then stores. */
  onCutToBin?: (passage: Passage) => void;
  debounceMs?: number;
}

export class Editor {
  private readonly view: EditorView;
  private readonly onChange: (markdown: string, words: number) => void;
  private readonly onCutToBin: (passage: Passage) => void;
  // Only scenes have a bin to cut to.
  private binEnabled = false;
  private readonly binKeys = keymap({ "Shift-Mod-x": () => this.cutToBin() });
  private readonly cutCallbacks: CutCallbacks;
  private readonly debounceMs: number;
  private typography: Typography;
  private spellchecker: Spellchecker | null = null;
  private linkResolver: LinkResolver | null = null;
  // Plugins with state are kept across reconfigures, so changing a setting doesn't wipe undo
  // history or spellcheck results.
  private readonly history = history();
  private readonly spellcheck = spellcheckPlugin(() => this.spellchecker);
  private readonly keymaps = buildKeymaps();
  // First, so Enter and Escape confirm or cancel a cut before the keymaps see them.
  private readonly cutLine = cutLinePlugin(() => this.cutCallbacks);
  private readonly seam = seamPlugin();
  private readonly links = linksPlugin(() => this.linkResolver);
  private readonly highlight = highlightPlugin();
  // Before the keymaps, so Enter picks a suggestion instead of splitting the paragraph.
  private readonly linkSuggest = linkSuggestPlugin(() => this.linkResolver);
  private timer: ReturnType<typeof setTimeout> | undefined;
  private seamTimer: ReturnType<typeof setTimeout> | undefined;
  private dirty = false;

  constructor(el: HTMLElement, markdown: string, options: MountOptions = {}) {
    this.onChange = options.onChange ?? (() => {});
    this.onCutToBin = options.onCutToBin ?? (() => {});
    this.cutCallbacks = {
      onCutConfirm: options.onCutConfirm ?? (() => {}),
      onCutCancel: options.onCutCancel ?? (() => {}),
    };
    this.debounceMs = options.debounceMs ?? 1000;
    this.typography = { ...defaultTypography, ...options.typography };
    this.view = new EditorView(el, {
      state: this.createState(parseMarkdown(markdown)),
      // WebKit's own spellchecker is off; see spellcheck.ts.
      attributes: { class: "needle-prose", spellcheck: "false", lang: "en-US" },
      dispatchTransaction: (tr) => {
        this.view.updateState(this.view.state.apply(tr));
        if (tr.docChanged) this.schedule();
      },
      handleDOMEvents: {
        blur: () => {
          this.flush();
          return false;
        },
        contextmenu: (view, event) =>
          (this.binEnabled && openSelectionMenu(view, event, () => this.cutToBin())) || openSpellMenu(view, event),
      },
    });
  }

  /** Loads a new document with fresh undo history. Pending changes are dropped, so callers
   * must `flush()` first when switching scenes. */
  setContent(markdown: string): void {
    this.cancel();
    this.view.updateState(this.createState(parseMarkdown(markdown)));
  }

  setOptions(options: EditorOptions): void {
    const typography = { ...this.typography, ...options.typography };
    if (Object.entries(typography).some(([key, on]) => this.typography[key as keyof Typography] !== on)) {
      this.typography = typography;
      this.view.updateState(this.view.state.reconfigure({ plugins: this.plugins() }));
    }
  }

  /** Starts (or, with null, stops) spellchecking the whole document. */
  setSpellchecker(checker: Spellchecker | null): void {
    this.spellchecker = checker;
    const meta: SpellMeta = { checker };
    this.view.dispatch(this.view.state.tr.setMeta(spellcheckKey, meta));
  }

  /** Starts (or, with null, stops) checking links against the notes there are. */
  setLinkResolver(resolver: LinkResolver | null): void {
    this.linkResolver = resolver;
    const meta: LinkMeta = { resolver };
    this.view.dispatch(this.view.state.tr.setMeta(linksKey, meta));
  }

  /** Checks every link again, after notes were made, renamed or given aliases. */
  refreshLinks(): void {
    const meta: LinkMeta = { refresh: true };
    this.view.dispatch(this.view.state.tr.setMeta(linksKey, meta));
  }

  /** Highlights words starting with any of `terms` (empty clears it), and with `reveal`,
   * scrolls the first one into view and puts the cursor there. */
  setHighlights(terms: string[], reveal: boolean): void {
    const meta: HighlightMeta = { terms };
    this.view.dispatch(this.view.state.tr.setMeta(highlightKey, meta));
    if (!reveal) return;
    const [first] = highlightKey.getState(this.view.state)?.decorations.find() ?? [];
    if (!first) return;
    const { state } = this.view;
    this.view.dispatch(state.tr.setSelection(TextSelection.create(state.doc, first.from)).scrollIntoView());
    this.view.dom.querySelector(".search-match")?.scrollIntoView({ block: "center" });
  }

  /** Checks every word again, after the spellchecker learned new ones. */
  recheckSpelling(): void {
    const meta: SpellMeta = { recheck: true };
    this.view.dispatch(this.view.state.tr.setMeta(spellcheckKey, meta));
  }

  /** Offers "Cut to bin" (and its shortcut) or not: on for scenes, off for notes. */
  setBinEnabled(on: boolean): void {
    this.binEnabled = on;
  }

  /** Takes the selected text out and hands it to `onCutToBin`. False if nothing's selected. */
  cutToBin(): boolean {
    if (!this.binEnabled) return false;
    const cut = cutPassage(this.view.state);
    if (!cut) return false;
    this.view.dispatch(cut.tr);
    this.view.focus();
    this.onCutToBin(cut.passage);
    return true;
  }

  /** Puts a passage from the bin back where it came from if that place is still there, and
   * otherwise at the cursor, then selects it. Returns whether it found the place. */
  restorePassage(passage: Passage): boolean {
    const { tr, atSpot } = restorePassage(this.view.state, passage);
    this.view.dispatch(tr);
    this.view.focus();
    return atSpot;
  }

  getMarkdown(): string {
    return serializeMarkdown(this.view.state.doc);
  }

  /** The document as Markdown before and after the cursor, for splitting a scene there. */
  splitParts(): { before: string; after: string } {
    const { doc, selection } = this.view.state;
    return {
      before: serializeMarkdown(doc.cut(0, selection.from)),
      after: serializeMarkdown(doc.cut(selection.from)),
    };
  }

  wordCount(): number {
    return countWords(this.view.state.doc);
  }

  /** Top-level blocks in the document (0 when it's empty), to place a seam after a merge. */
  blockCount(): number {
    return blockCount(this.view.state.doc);
  }

  /** Shows or hides the cut line at the cursor, which previews where Split will cut. While it
   * shows, Enter and Escape call `onCutConfirm` and `onCutCancel`. */
  showCutLine(on: boolean): void {
    this.view.dispatch(this.view.state.tr.setMeta(cutLineKey, on).scrollIntoView());
    if (on) this.view.focus();
  }

  /** Marks where a merged scene was joined on, before top-level block `index`, and puts the
   * cursor there. The seam fades by itself. */
  showSeam(index: number): void {
    const { state } = this.view;
    const pos = seamPosition(state.doc, index);
    const meta: SeamMeta = pos;
    this.view.dispatch(state.tr.setMeta(seamKey, meta).setSelection(Selection.near(state.doc.resolve(pos))));
    this.view.dom.querySelector(".seam")?.scrollIntoView({ block: "center", behavior: "smooth" });
    clearTimeout(this.seamTimer);
    this.seamTimer = setTimeout(() => {
      const clear: SeamMeta = "clear";
      this.view.dispatch(this.view.state.tr.setMeta(seamKey, clear));
    }, SEAM_MS);
  }

  focus(): void {
    this.view.focus();
  }

  /** Reports pending changes immediately instead of waiting for the debounce. */
  flush(): void {
    if (!this.dirty) return;
    this.cancel();
    this.onChange(this.getMarkdown(), this.wordCount());
  }

  destroy(): void {
    this.flush();
    clearTimeout(this.seamTimer);
    this.view.destroy();
  }

  private plugins(): Plugin[] {
    return [
      this.cutLine,
      this.linkSuggest,
      this.binKeys,
      buildInputRules(this.typography),
      ...this.keymaps,
      this.history,
      this.spellcheck,
      this.links,
      this.highlight,
      this.seam,
    ];
  }

  private createState(doc: Node): EditorState {
    return EditorState.create({ doc, plugins: this.plugins() });
  }

  private schedule(): void {
    this.dirty = true;
    clearTimeout(this.timer);
    this.timer = setTimeout(() => this.flush(), this.debounceMs);
  }

  private cancel(): void {
    clearTimeout(this.timer);
    this.timer = undefined;
    this.dirty = false;
  }
}

export function mount(el: HTMLElement, markdown: string, options?: MountOptions): Editor {
  return new Editor(el, markdown, options);
}
