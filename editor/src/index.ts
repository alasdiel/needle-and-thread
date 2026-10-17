// The editor island. Bundled as an IIFE exposing `window.NeedleEditor`; the Rust side binds to
// it in crates/desktop/src/editor.rs. ProseMirror owns the text while typing, Rust owns files.

import "./style.css";
import type { Node } from "prosemirror-model";
import { closeHistory, history, isHistoryTransaction } from "prosemirror-history";
import { keymap } from "prosemirror-keymap";
import { EditorState, type Plugin, Selection, TextSelection } from "prosemirror-state";
import { EditorView } from "prosemirror-view";
import { type Move, type Passage, cutPassage, followHistory, restorePassage } from "./bin.ts";
import { type Typography, buildInputRules, defaultTypography } from "./inputrules.ts";
import { buildKeymaps } from "./keymap.ts";
import { type HighlightMeta, highlightKey, highlightPlugin } from "./highlight.ts";
import { type LinkMeta, type LinkResolver, type ShowSuggestions, linkSuggestPlugin, linksKey, linksPlugin, pickSuggestion } from "./links.ts";
import { countWords, parseMarkdown, serializeMarkdown } from "./markdown.ts";
import { type CommentMark, type CommentsMeta, type OnComment, commentsKey, commentsPlugin } from "./comments.ts";
import { type Choice, type MergeBox, type MergeMeta, mergeKey, mergePlugin } from "./merge.ts";
import { openSelectionMenu, openSpellMenu } from "./menu.ts";
import { changeTo } from "./putback.ts";
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
import { type Format, activeFormats, formatCommands } from "./touch.ts";

export type { Choice, CommentMark, Format, LinkResolver, MergeBox, Passage, ShowSuggestions, Spellchecker, Typography };
export { hasSpot } from "./bin.ts";

export interface EditorOptions {
  typography?: Partial<Typography>;
}

export interface MountOptions extends EditorOptions, Partial<CutCallbacks> {
  /** Called with the document's Markdown once typing pauses for `debounceMs`. */
  onChange?: (markdown: string, words: number) => void;
  /** Called with a passage just cut to the bin, which the app then stores, and the id of that
   * move. Undo after a restore calls it too, as that takes the passage out again. */
  onCutToBin?: (passage: Passage, move: number) => void;
  /** Called when Undo (or Redo) puts a binned passage back in the text, with the id `onCutToBin`
   * had, so the app takes it out of the bin. */
  onBackFromBin?: (move: number) => void;
  /** Called when the writer settles a clash shown with `setMerges`, with its id. */
  onMerge?: (id: number, choice: Choice) => void;
  /** Called when the writer clicks words a reader commented on, with the comment's id and the
   * top and bottom of the words in the window. */
  onComment?: OnComment;
  debounceMs?: number;
  /** For a phone: the keyboard's own spellcheck and autocorrect instead of the app's, no menus
   * over the text, and the cursor kept clear of the window's edges. */
  touch?: boolean;
  /** Called with the formats on where the cursor is, whenever they change, for a toolbar. */
  onFormats?: (on: Format[]) => void;
  /** Shows suggestions while typing a link, instead of the list under the cursor. */
  onSuggestions?: ShowSuggestions;
}

export class Editor {
  private readonly view: EditorView;
  private readonly onChange: (markdown: string, words: number) => void;
  private readonly onCutToBin: (passage: Passage, move: number) => void;
  private readonly onBackFromBin: (move: number) => void;
  private readonly onMerge: (id: number, choice: Choice) => void;
  private readonly onComment: OnComment;
  private readonly touch: boolean;
  private readonly onFormats: ((on: Format[]) => void) | null;
  private readonly onSuggestions: ShowSuggestions | null;
  private formatsShown = "";
  // Passages moved to and from the bin in this document, newest last, for Undo and Redo.
  private moves: Move[] = [];
  private lastMove = 0;
  // Starts a new undo step with the next edit, so a move is always a step of its own.
  private closeNext = false;
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
  private readonly links = linksPlugin(
    () => this.linkResolver,
    () => this.touch,
  );
  private readonly highlight = highlightPlugin();
  private readonly merge = mergePlugin(() => this.onMerge);
  private readonly comments = commentsPlugin(() => this.onComment);
  // Before the keymaps, so Enter picks a suggestion instead of splitting the paragraph.
  private readonly linkSuggest = linkSuggestPlugin(
    () => this.linkResolver,
    () => this.onSuggestions,
  );
  private timer: ReturnType<typeof setTimeout> | undefined;
  private seamTimer: ReturnType<typeof setTimeout> | undefined;
  private dirty = false;

  constructor(el: HTMLElement, markdown: string, options: MountOptions = {}) {
    this.onChange = options.onChange ?? (() => {});
    this.onCutToBin = options.onCutToBin ?? (() => {});
    this.onBackFromBin = options.onBackFromBin ?? (() => {});
    this.onMerge = options.onMerge ?? (() => {});
    this.onComment = options.onComment ?? (() => {});
    this.touch = options.touch ?? false;
    this.onFormats = options.onFormats ?? null;
    this.onSuggestions = options.onSuggestions ?? null;
    this.cutCallbacks = {
      onCutConfirm: options.onCutConfirm ?? (() => {}),
      onCutCancel: options.onCutCancel ?? (() => {}),
    };
    this.debounceMs = options.debounceMs ?? 1000;
    this.typography = { ...defaultTypography, ...options.typography };
    this.view = new EditorView(el, {
      state: this.createState(parseMarkdown(markdown)),
      // WebKit's own spellchecker is off on the desktop; see spellcheck.ts. A phone's keyboard
      // checks as you type, which is what's wanted there.
      attributes: this.touch
        ? { class: "needle-prose touch", spellcheck: "true", autocorrect: "on", autocapitalize: "sentences", lang: "en-US" }
        : { class: "needle-prose", spellcheck: "false", lang: "en-US" },
      // Room for a line or two above the keyboard and the bar.
      scrollMargin: this.touch ? 48 : 5,
      scrollThreshold: this.touch ? 48 : 0,
      dispatchTransaction: (tr) => {
        if (this.closeNext && tr.docChanged) {
          closeHistory(tr);
          this.closeNext = false;
        }
        this.view.updateState(this.view.state.apply(tr));
        this.reportFormats();
        if (tr.docChanged) {
          this.schedule();
          if (isHistoryTransaction(tr)) this.followHistory();
        }
      },
      handleDOMEvents: {
        blur: () => {
          this.flush();
          return false;
        },
        contextmenu: (view, event) =>
          !this.touch &&
          (this.binEnabled && openSelectionMenu(view, event, () => this.cutToBin())) || openSpellMenu(view, event),
      },
    });
  }

  /** Loads a new document with fresh undo history. Pending changes are dropped, so callers
   * must `flush()` first when switching scenes. */
  setContent(markdown: string): void {
    this.cancel();
    this.moves = [];
    this.closeNext = false;
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

  /** Shows where paragraphs were changed here and on another device too, each with the other
   * device's version under it (empty clears them). */
  setMerges(boxes: MergeBox[]): void {
    const meta: MergeMeta = { boxes };
    this.view.dispatch(this.view.state.tr.setMeta(mergeKey, meta));
  }

  /** Marks the words readers commented on (an empty list clears them). Rust says where they
   * are; see comments.ts. */
  setComments(marks: CommentMark[]): void {
    const meta: CommentsMeta = { marks };
    this.view.dispatch(this.view.state.tr.setMeta(commentsKey, meta));
  }

  /** Draws one comment's words stronger, while its card is open (null for none). */
  setActiveComment(id: number | null): void {
    const meta: CommentsMeta = { active: id };
    this.view.dispatch(this.view.state.tr.setMeta(commentsKey, meta));
  }

  /** Scrolls a comment's words into view, and returns their top and bottom in the window, or
   * null if they aren't marked. */
  revealComment(id: number): { top: number; bottom: number } | null {
    const marks = Array.from(this.view.dom.querySelectorAll(`.comment-mark[data-comment="${id}"]`));
    if (marks.length === 0) return null;
    marks[0].scrollIntoView({ block: "center" });
    const rects = marks.map((el) => el.getBoundingClientRect());
    return { top: Math.min(...rects.map((r) => r.top)), bottom: Math.max(...rects.map((r) => r.bottom)) };
  }

  /** Scrolls the clash with this id into view, and puts the cursor at its start. */
  revealMerge(id: number): void {
    const box = this.view.dom.querySelector(`.merge-other[data-merge="${id}"]`);
    if (!box) return;
    const mine = box.previousElementSibling?.classList.contains("merge-mine") ? box.previousElementSibling : null;
    const target = mine ?? box;
    const pos = this.view.posAtDOM(target, 0);
    const { state } = this.view;
    this.view.dispatch(state.tr.setSelection(Selection.near(state.doc.resolve(Math.min(pos, state.doc.content.size)))));
    this.view.focus();
    target.scrollIntoView({ block: "center", behavior: "smooth" });
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
    const withIt = this.view.state.doc;
    this.view.dispatch(closeHistory(cut.tr));
    const id = this.remember(cut.passage, withIt, true);
    this.view.focus();
    this.onCutToBin(cut.passage, id);
    return true;
  }

  /** Puts a passage from the bin back where it came from if that place is still there, and
   * otherwise at the cursor, then selects it. Returns whether it found the place. */
  restorePassage(passage: Passage): boolean {
    const without = this.view.state.doc;
    const { tr, atSpot } = restorePassage(this.view.state, passage);
    this.view.dispatch(closeHistory(tr));
    this.remember(passage, without, false);
    this.view.focus();
    return atSpot;
  }

  /** Changes the document to `markdown` in one edit (so Undo takes it back) that touches only
   * what differs, and selects that. For putting a passage back from History. Returns whether
   * anything changed. */
  applyMarkdown(markdown: string): boolean {
    const tr = changeTo(this.view.state, parseMarkdown(markdown));
    if (!tr) return false;
    this.view.dispatch(tr);
    this.view.focus();
    return true;
  }

  /** Changes the document to `markdown`, touching only what differs and leaving the cursor and
   * scroll where they were. For text that changed on another device: it isn't the writer's own
   * edit, so Undo doesn't take it back. Returns whether anything changed. */
  replaceMarkdown(markdown: string): boolean {
    const tr = changeTo(this.view.state, parseMarkdown(markdown), false);
    if (!tr) return false;
    this.view.dispatch(tr.setMeta("addToHistory", false));
    return true;
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

  /** Lets go of the cursor, which on a phone puts the keyboard away. */
  blur(): void {
    (this.view.dom as HTMLElement).blur();
  }

  /** Runs a formatting bar button. Returns whether it did anything. */
  format(name: Format): boolean {
    const command = formatCommands[name];
    if (!command) return false;
    const done = command(this.view.state, this.view.dispatch);
    this.view.focus();
    return done;
  }

  /** The formats on where the cursor is. */
  formats(): Format[] {
    return activeFormats(this.view.state);
  }

  /** Takes the `index`th suggestion `onSuggestions` was given. */
  pickSuggestion(index: number): void {
    pickSuggestion(this.view, index);
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
      this.merge,
      this.comments,
    ];
  }

  /** Notes a move just made, `other` being the document on its other side. Returns its id. */
  private remember(passage: Passage, other: Node, inBin: boolean): number {
    const now = this.view.state.doc;
    const id = ++this.lastMove;
    this.moves.push({ id, passage, inBin, with: inBin ? other : now, without: inBin ? now : other });
    if (this.moves.length > 50) this.moves.shift();
    this.closeNext = true;
    return id;
  }

  /** After Undo or Redo, takes passages that are back in the text out of the bin, and puts
   * those that are out of it again back in. */
  private followHistory(): void {
    const { back, out } = followHistory(this.moves, this.view.state.doc);
    for (const move of back) this.onBackFromBin(move.id);
    for (const move of out) this.onCutToBin(move.passage, move.id);
  }

  private reportFormats(): void {
    if (!this.onFormats) return;
    const on = activeFormats(this.view.state);
    const key = on.join(" ");
    if (key === this.formatsShown) return;
    this.formatsShown = key;
    this.onFormats(on);
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
