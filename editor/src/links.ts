// Note links in the text: how each [[link]] looks (a note found, none yet, or two that share
// the name), what clicking one does, and suggestions while typing after `[[`. Finding notes is
// Rust's job (needle_core::names, through the wasm frontend); this decides what to ask, and when.

import type { Node } from "prosemirror-model";
import { type EditorState, Plugin, PluginKey, TextSelection, type Transaction } from "prosemirror-state";
import { Decoration, DecorationSet, type EditorView } from "prosemirror-view";
import { type MenuItem, showMenu } from "./menu.ts";
import { schema } from "./schema.ts";

export type LinkState = "found" | "loose" | "ambiguous";

export interface LinkTarget {
  state: LinkState;
  /** The note found, or the notes that share the name. `kind` is a label, e.g. "Character". */
  notes: { title: string; kind: string }[];
}

export interface LinkSuggestion {
  title: string;
  /** The title or alias that matched; an alias becomes the link's shown text. */
  name: string;
  alias: boolean;
  kind: string;
}

export interface LinkResolver {
  resolve(target: string): LinkTarget;
  /** Notes whose title or an alias matches `query`, best first. */
  suggest(query: string): LinkSuggestion[];
  /** Opens the `index`th note `target` finds. */
  open(target: string, index: number): void;
  /** The types of note that can be made, as [type, label] pairs. */
  kinds(): string[][];
  /** Makes a note called `title`. */
  create(title: string, kind: string): void;
}

export type LinkMeta = { resolver: LinkResolver | null } | { refresh: true };

interface LinkPluginState {
  resolver: LinkResolver | null;
  decorations: DecorationSet;
}

export const linksKey = new PluginKey<LinkPluginState>("links");

const SUGGESTIONS = 8;

const LOOSE_TITLE = "No note yet. Click to make one.";
const AMBIGUOUS_TITLE = "More than one note has this name. Click to choose.";

/** Marks links to notes that don't exist yet (basted, not sewn) or that two notes share. */
export function linksPlugin(currentResolver: () => LinkResolver | null): Plugin<LinkPluginState> {
  return new Plugin<LinkPluginState>({
    key: linksKey,
    state: {
      init: (_config, state) => decorate(currentResolver(), state.doc),
      apply(tr, prev, _old, state) {
        const meta = tr.getMeta(linksKey) as LinkMeta | undefined;
        if (meta && "resolver" in meta) return decorate(meta.resolver, state.doc);
        if (meta || tr.docChanged) return decorate(prev.resolver, state.doc);
        return prev;
      },
    },
    props: {
      decorations: (state) => linksKey.getState(state)?.decorations,
      handleClickOn(view, _pos, node, nodePos, event, direct) {
        const resolver = linksKey.getState(view.state)?.resolver;
        if (!direct || node.type !== schema.nodes.wikilink || !resolver) return false;
        const target: string = node.attrs.target;
        const found = resolver.resolve(target);
        // Ctrl-click (Cmd on a Mac) opens a found note straight away.
        if (found.state === "found" && (event.ctrlKey || event.metaKey)) {
          resolver.open(target, 0);
          return true;
        }
        // The cursor goes after the link rather than selecting it, so typing next can't
        // replace it.
        const after = TextSelection.create(view.state.doc, nodePos + node.nodeSize);
        view.dispatch(view.state.tr.setSelection(after));
        const dom = view.nodeDOM(nodePos);
        const rect = dom instanceof HTMLElement ? dom.getBoundingClientRect() : null;
        showMenu(view, rect?.left ?? event.clientX, (rect?.bottom ?? event.clientY) + 4, linkMenu(resolver, target, found));
        return true;
      },
    },
  });
}

function decorate(resolver: LinkResolver | null, doc: Node): LinkPluginState {
  if (!resolver) return { resolver, decorations: DecorationSet.empty };
  const found: Decoration[] = [];
  const seen = new Map<string, LinkState>();
  doc.descendants((node, pos) => {
    if (node.type !== schema.nodes.wikilink) return node.isBlock;
    const target: string = node.attrs.target;
    let state = seen.get(target);
    if (state === undefined) {
      state = resolver.resolve(target).state;
      seen.set(target, state);
    }
    if (state === "loose") found.push(Decoration.node(pos, pos + node.nodeSize, { class: "loose", title: LOOSE_TITLE }));
    if (state === "ambiguous") {
      found.push(Decoration.node(pos, pos + node.nodeSize, { class: "ambiguous", title: AMBIGUOUS_TITLE }));
    }
    return false;
  });
  return { resolver, decorations: DecorationSet.create(doc, found) };
}

/** What clicking a link offers: open its note, make one, or choose between those sharing it. */
export function linkMenu(resolver: LinkResolver, target: string, found: LinkTarget): (MenuItem | "separator")[] {
  switch (found.state) {
    case "found":
      return [{ label: `Open “${found.notes[0].title}”`, run: () => resolver.open(target, 0) }];
    case "ambiguous":
      return [
        { label: `“${target}” could be:` },
        ...found.notes.map((note, i) => ({ label: `${note.title} · ${note.kind}`, run: () => resolver.open(target, i) })),
      ];
    case "loose":
      return [
        { label: `No note for “${target}” yet` },
        "separator",
        ...resolver.kinds().map(([kind, label]) => ({ label: `New ${label.toLowerCase()}`, run: () => resolver.create(target, kind) })),
      ];
  }
}

// --- Suggestions after `[[` ------------------------------------------------------------------

/** The `[[` being typed before the cursor, and what follows it so far. */
export function linkQuery(state: EditorState): { from: number; query: string } | null {
  const { selection } = state;
  const $from = selection.$from;
  if (!selection.empty || !$from.parent.isTextblock) return null;
  // Inline nodes count as one character each; "￼" keeps them from looking like text.
  const before = $from.parent.textBetween(0, $from.parentOffset, undefined, "￼");
  const match = /\[\[([^[\]|\n￼]{0,60})$/.exec(before);
  return match ? { from: $from.pos - match[0].length, query: match[1] } : null;
}

/** Replaces the `[[query` before the cursor with a link: to the note's title, showing the alias
 * if an alias was picked. */
export function insertLink(state: EditorState, from: number, title: string, shown: string | null): Transaction {
  const link = schema.nodes.wikilink.create({ target: title, label: shown && shown !== title ? shown : null });
  return state.tr.replaceWith(from, state.selection.from, link).scrollIntoView();
}

interface Choice {
  title: string;
  shown: string | null;
  label: string;
  detail: string;
}

/** The list under the cursor while typing a link. Arrow keys move, Enter or Tab picks, Escape
 * closes it until the `[[` is gone. */
class Suggestions {
  private readonly view: EditorView;
  private readonly resolver: () => LinkResolver | null;
  private readonly el: HTMLDivElement;
  private choices: Choice[] = [];
  private selected = 0;
  private from = -1;
  private dismissedAt = -1;

  constructor(view: EditorView, resolver: () => LinkResolver | null) {
    this.view = view;
    this.resolver = resolver;
    this.el = document.createElement("div");
    this.el.className = "needle-menu needle-suggest";
    this.el.setAttribute("role", "listbox");
    this.el.hidden = true;
    document.body.append(this.el);
  }

  get open(): boolean {
    return !this.el.hidden;
  }

  update(): void {
    const resolver = this.resolver();
    const found = resolver && this.view.hasFocus() ? linkQuery(this.view.state) : null;
    if (!found || !resolver) {
      this.dismissedAt = -1;
      return this.hide();
    }
    if (found.from === this.dismissedAt) return this.hide();

    const choices: Choice[] = resolver.suggest(found.query).slice(0, SUGGESTIONS).map((s) => ({
      title: s.title,
      shown: s.alias ? s.name : null,
      label: s.name,
      detail: s.alias ? `${s.title} · ${s.kind}` : s.kind,
    }));
    const query = found.query.trim();
    const exact = choices.some((c) => c.label.toLowerCase() === query.toLowerCase());
    if (query && !exact) choices.push({ title: query, shown: null, label: `“${query}”`, detail: "no note yet" });
    if (!choices.length) return this.hide();

    if (found.from !== this.from) this.selected = 0;
    this.from = found.from;
    this.choices = choices;
    this.selected = Math.min(this.selected, choices.length - 1);
    this.render();
  }

  /** Returns true if the key was the list's to handle. */
  keydown(event: KeyboardEvent): boolean {
    if (!this.open) return false;
    switch (event.key) {
      case "ArrowDown":
      case "ArrowUp": {
        const step = event.key === "ArrowDown" ? 1 : -1;
        this.selected = (this.selected + step + this.choices.length) % this.choices.length;
        this.render();
        return true;
      }
      case "Enter":
      case "Tab":
        this.pick(this.selected);
        return true;
      case "Escape":
        this.dismissedAt = this.from;
        this.hide();
        return true;
      default:
        return false;
    }
  }

  destroy(): void {
    this.el.remove();
  }

  private pick(index: number): void {
    const choice = this.choices[index];
    if (!choice) return;
    this.hide();
    this.view.dispatch(insertLink(this.view.state, this.from, choice.title, choice.shown));
    this.view.focus();
  }

  private hide(): void {
    this.el.hidden = true;
    this.choices = [];
  }

  private render(): void {
    this.el.replaceChildren(
      ...this.choices.map((choice, i) => {
        const item = document.createElement("button");
        item.type = "button";
        item.setAttribute("role", "option");
        item.setAttribute("aria-selected", String(i === this.selected));
        item.tabIndex = -1;
        const name = document.createElement("span");
        name.textContent = choice.label;
        const detail = document.createElement("span");
        detail.className = "detail";
        detail.textContent = choice.detail;
        item.append(name, detail);
        // On mousedown, so the editor keeps the cursor.
        item.addEventListener("mousedown", (e) => {
          e.preventDefault();
          this.pick(i);
        });
        return item;
      }),
    );
    this.el.hidden = false;
    const at = this.view.coordsAtPos(this.from);
    const { width, height } = this.el.getBoundingClientRect();
    const below = at.bottom + 4 + height < window.innerHeight;
    this.el.style.left = `${Math.max(4, Math.min(at.left, window.innerWidth - width - 4))}px`;
    this.el.style.top = `${below ? at.bottom + 4 : Math.max(4, at.top - height - 4)}px`;
  }
}

const suggestionsByView = new WeakMap<EditorView, Suggestions>();

/** Suggests notes after `[[`. Goes before the keymaps, so Enter picks rather than splitting. */
export function linkSuggestPlugin(currentResolver: () => LinkResolver | null): Plugin {
  return new Plugin({
    view(view) {
      const suggestions = new Suggestions(view, currentResolver);
      suggestionsByView.set(view, suggestions);
      return {
        update: () => suggestions.update(),
        destroy: () => suggestions.destroy(),
      };
    },
    props: {
      handleKeyDown: (view, event) => suggestionsByView.get(view)?.keydown(event) ?? false,
      handleDOMEvents: {
        blur(view) {
          suggestionsByView.get(view)?.update();
          return false;
        },
      },
    },
  });
}
