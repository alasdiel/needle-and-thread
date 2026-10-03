// Spellcheck underlines, drawn as decorations. The checking itself happens in Rust
// (needle_core::spell, via the wasm frontend); this decides what to check, and when.
//
// WebKit's built-in spellchecker only looks at text as it's typed, so a scene opened with
// mistakes in it showed none of them. This checks the whole document on load instead.

import type { Node } from "prosemirror-model";
import { type EditorState, Plugin, PluginKey, type Transaction } from "prosemirror-state";
import { Decoration, DecorationSet } from "prosemirror-view";

export interface Spellchecker {
  check(word: string): boolean;
  suggest(word: string): string[];
  /** Accepts `word` from now on, and remembers it. */
  add(word: string): void;
}

export interface SpellState {
  session: Session | null;
  decorations: DecorationSet;
}

export type SpellMeta = { checker: Spellchecker | null } | { recheck: true };

export const spellcheckKey = new PluginKey<SpellState>("spellcheck");

// Letters with internal apostrophes ("don't", "Mara’s"). Numbers and symbols are never checked.
const WORD = /[\p{L}\p{M}]+(?:['’][\p{L}\p{M}]+)*/gu;
// Stands in for inline nodes (wikilinks, line breaks), so text offsets match document positions.
const OBJECT = "￼";

export class Session {
  readonly checker: Spellchecker;
  /** Words from the document's [[wikilinks]]: character and place names count as spelled right. */
  known: ReadonlySet<string>;
  private readonly cache = new Map<string, boolean>();

  constructor(checker: Spellchecker, known: ReadonlySet<string>) {
    this.checker = checker;
    this.known = known;
  }

  isCorrect(word: string): boolean {
    if (this.known.has(word) || this.known.has(word.replace(/['’][sS]$/, ""))) return true;
    let correct = this.cache.get(word);
    if (correct === undefined) {
      correct = this.checker.check(word);
      this.cache.set(word, correct);
    }
    return correct;
  }

  clearCache(): void {
    this.cache.clear();
  }
}

/** `currentChecker` is read whenever a new document is loaded, so the checker survives scene switches. */
export function spellcheckPlugin(currentChecker: () => Spellchecker | null): Plugin<SpellState> {
  return new Plugin<SpellState>({
    key: spellcheckKey,
    state: {
      init: (_config, state) => start(currentChecker(), state.doc),

      apply(tr, prev, oldState, newState) {
        const meta = tr.getMeta(spellcheckKey) as SpellMeta | undefined;
        if (meta && "checker" in meta) return start(meta.checker, newState.doc);

        const { session } = prev;
        if (!session) return prev;
        if (meta && "recheck" in meta) {
          session.clearCache();
          return { session, decorations: checkAll(newState.doc, session, -1) };
        }
        if (!tr.docChanged && !tr.selectionSet) return prev;

        // Don't flag a word while it's still being typed; it's checked once the cursor leaves it.
        const typingAt = tr.docChanged && tr.selection.empty ? tr.selection.head : -1;

        if (tr.docChanged) {
          const names = namesIn(newState.doc);
          if (!sameWords(names, session.known)) {
            session.known = names;
            return { session, decorations: checkAll(newState.doc, session, typingAt) };
          }
        }

        let decorations = prev.decorations.map(tr.mapping, tr.doc);
        for (const pos of blocksToRecheck(tr, oldState)) {
          const block = tr.doc.nodeAt(pos);
          if (!block) continue;
          const start = pos + 1;
          decorations = decorations
            .remove(decorations.find(start, start + block.content.size))
            .add(tr.doc, misspellingsIn(block, start, session, typingAt));
        }
        return { session, decorations };
      },
    },
    props: {
      decorations: (state) => spellcheckKey.getState(state)?.decorations,
    },
  });
}

function start(checker: Spellchecker | null, doc: Node): SpellState {
  if (!checker) return { session: null, decorations: DecorationSet.empty };
  const session = new Session(checker, namesIn(doc));
  return { session, decorations: checkAll(doc, session, -1) };
}

function checkAll(doc: Node, session: Session, typingAt: number): DecorationSet {
  const found: Decoration[] = [];
  doc.descendants((node, pos) => {
    if (!node.isTextblock) return true;
    found.push(...misspellingsIn(node, pos + 1, session, typingAt));
    return false;
  });
  return DecorationSet.create(doc, found);
}

function misspellingsIn(block: Node, start: number, session: Session, typingAt: number): Decoration[] {
  let text = "";
  block.forEach((child) => {
    if (!child.isText) text += OBJECT;
    else if (isUrl(child)) text += " ".repeat(child.text!.length);
    else text += child.text;
  });

  const found: Decoration[] = [];
  for (const match of text.matchAll(WORD)) {
    const word = match[0];
    const from = start + match.index;
    const to = from + word.length;
    if (word.length < 2 || (typingAt >= from && typingAt <= to)) continue;
    if (!session.isCorrect(word)) found.push(Decoration.inline(from, to, { class: "misspelled" }, { word }));
  }
  return found;
}

function isUrl(text: Node): boolean {
  return text.marks.some((m) => m.type.name === "link") && /^[a-z][\w+.-]*:\/\//i.test(text.text!);
}

function namesIn(doc: Node): Set<string> {
  const names = new Set<string>();
  doc.descendants((node) => {
    if (node.type.name === "wikilink") {
      for (const part of [node.attrs.target, node.attrs.label]) {
        for (const [word] of String(part ?? "").matchAll(WORD)) names.add(word);
      }
    }
    return node.isBlock;
  });
  return names;
}

function sameWords(a: ReadonlySet<string>, b: ReadonlySet<string>): boolean {
  return a.size === b.size && [...a].every((w) => b.has(w));
}

/** Positions of the textblocks a transaction touched, plus those the cursor left or entered. */
function blocksToRecheck(tr: Transaction, oldState: EditorState): Set<number> {
  const blocks = new Set<number>();
  const addBlockAt = (pos: number) => {
    const $pos = tr.doc.resolve(Math.max(0, Math.min(pos, tr.doc.content.size)));
    if ($pos.parent.isTextblock) blocks.add($pos.before());
  };

  tr.mapping.maps.forEach((map, i) => {
    const later = tr.mapping.slice(i + 1);
    map.forEach((_oldStart, _oldEnd, newStart, newEnd) => {
      const from = later.map(newStart, -1);
      const to = later.map(newEnd, 1);
      addBlockAt(from);
      addBlockAt(to);
      tr.doc.nodesBetween(from, to, (node, pos) => {
        if (!node.isTextblock) return true;
        blocks.add(pos);
        return false;
      });
    });
  });
  addBlockAt(tr.selection.head);
  addBlockAt(tr.mapping.map(oldState.selection.head));
  return blocks;
}
