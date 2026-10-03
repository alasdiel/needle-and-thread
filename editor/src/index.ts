// The editor island. Bundled as an IIFE exposing `window.NeedleEditor`; the Rust side binds to
// it in crates/desktop/src/editor.rs. ProseMirror owns the text while typing, Rust owns files.

import "./style.css";
import type { Node } from "prosemirror-model";
import { history } from "prosemirror-history";
import { EditorState, type Plugin } from "prosemirror-state";
import { EditorView } from "prosemirror-view";
import { type Typography, buildInputRules, defaultTypography } from "./inputrules.ts";
import { buildKeymaps } from "./keymap.ts";
import { countWords, parseMarkdown, serializeMarkdown } from "./markdown.ts";
import { openSpellMenu } from "./menu.ts";
import { type SpellMeta, type Spellchecker, spellcheckKey, spellcheckPlugin } from "./spellcheck.ts";

export type { Spellchecker, Typography };

export interface EditorOptions {
  typography?: Partial<Typography>;
}

export interface MountOptions extends EditorOptions {
  /** Called with the document's Markdown once typing pauses for `debounceMs`. */
  onChange?: (markdown: string, words: number) => void;
  debounceMs?: number;
}

export class Editor {
  private readonly view: EditorView;
  private readonly onChange: (markdown: string, words: number) => void;
  private readonly debounceMs: number;
  private typography: Typography;
  private spellchecker: Spellchecker | null = null;
  // Plugins with state are kept across reconfigures, so changing a setting doesn't wipe undo
  // history or spellcheck results.
  private readonly history = history();
  private readonly spellcheck = spellcheckPlugin(() => this.spellchecker);
  private readonly keymaps = buildKeymaps();
  private timer: ReturnType<typeof setTimeout> | undefined;
  private dirty = false;

  constructor(el: HTMLElement, markdown: string, options: MountOptions = {}) {
    this.onChange = options.onChange ?? (() => {});
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
        contextmenu: (view, event) => openSpellMenu(view, event),
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
    this.view.destroy();
  }

  private plugins(): Plugin[] {
    return [buildInputRules(this.typography), ...this.keymaps, this.history, this.spellcheck];
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
