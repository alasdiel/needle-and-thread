// Readers' comments on shared text (DESIGN §10). Rust works out where each comment's words are
// now (core's `anchor`) and hands over the words and which of their occurrences it is; this
// finds them in the document, counted the same way, and marks them. Clicking marked words
// calls back with the comment's id and where they are, for the app to open its card beside
// them. Decorations only: comments never change the text.

import type { Node } from "prosemirror-model";
import { Plugin, PluginKey } from "prosemirror-state";
import { Decoration, DecorationSet } from "prosemirror-view";

/** A comment's words: `quote` with its spaces folded, and how many times it comes earlier. */
export interface CommentMark {
  id: number;
  quote: string;
  n: number;
}

export interface CommentsMeta {
  marks?: CommentMark[];
  /** The comment whose card is open, drawn stronger; null for none. */
  active?: number | null;
}

interface CommentsState {
  marks: CommentMark[];
  active: number | null;
  decorations: DecorationSet;
}

export const commentsKey = new PluginKey<CommentsState>("comments");

/** The document's text the way the anchors count it, spaces folded: every run of spaces and
 * line breaks (and the break between blocks) is one space, a wikilink is its label. With each
 * character's place in the document, from and to. */
export function proseOf(doc: Node): { chars: string[]; from: number[]; to: number[] } {
  const chars: string[] = [];
  const from: number[] = [];
  const to: number[] = [];
  const push = (text: string, start: number, single: boolean) => {
    let i = 0;
    for (const c of text) {
      const at = single ? start : start + i;
      i += c.length;
      if (/\s/u.test(c)) {
        if (chars[chars.length - 1] === " ") continue;
        chars.push(" ");
      } else {
        chars.push(c);
      }
      from.push(at);
      to.push(single ? start + 1 : start + i);
    }
  };
  doc.descendants((node, pos) => {
    if (node.isTextblock) {
      node.forEach((child, offset) => {
        const at = pos + 1 + offset;
        if (child.isText) push(child.text!, at, false);
        else if (child.type.name === "hard_break") push(" ", at, true);
        else if (child.isLeaf) push(child.type.spec.leafText?.(child) ?? "", at, true);
      });
      push("\n", pos + node.nodeSize - 1, true);
      return false;
    }
    return true;
  });
  return { chars, from, to };
}

/** Where a comment's words are in the document, or null if they aren't there. */
export function findMark(doc: Node, mark: CommentMark): { from: number; to: number } | null {
  const prose = proseOf(doc);
  const quote = Array.from(mark.quote);
  if (quote.length === 0) return null;
  let seen = 0;
  for (let i = 0; i + quote.length <= prose.chars.length; i++) {
    let same = true;
    for (let j = 0; j < quote.length && same; j++) same = prose.chars[i + j] === quote[j];
    if (!same) continue;
    if (seen === mark.n) return { from: prose.from[i], to: prose.to[i + quote.length - 1] };
    seen++;
  }
  return null;
}

function decorate(doc: Node, marks: CommentMark[], active: number | null): DecorationSet {
  const found: Decoration[] = [];
  for (const mark of marks) {
    const at = findMark(doc, mark);
    if (!at) continue;
    const className = mark.id === active ? "comment-mark active" : "comment-mark";
    found.push(Decoration.inline(at.from, at.to, { class: className, "data-comment": String(mark.id) }));
  }
  return DecorationSet.create(doc, found);
}

export type OnComment = (id: number, top: number, bottom: number) => void;

export function commentsPlugin(onComment: () => OnComment): Plugin<CommentsState> {
  return new Plugin<CommentsState>({
    key: commentsKey,
    state: {
      init: () => ({ marks: [], active: null, decorations: DecorationSet.empty }),
      apply(tr, state) {
        const meta = tr.getMeta(commentsKey) as CommentsMeta | undefined;
        if (meta) {
          const marks = meta.marks ?? state.marks;
          const active = meta.active === undefined ? state.active : meta.active;
          return { marks, active, decorations: decorate(tr.doc, marks, active) };
        }
        // Marked words move with the text as it's typed, until Rust places them again.
        if (!tr.docChanged) return state;
        return { ...state, decorations: state.decorations.map(tr.mapping, tr.doc) };
      },
    },
    props: {
      decorations: (state) => commentsKey.getState(state)?.decorations,
      handleClick(view, _pos, event) {
        // A click that selects something isn't a click on a comment.
        if (!view.state.selection.empty) return false;
        const target = (event.target as HTMLElement | null)?.closest?.(".comment-mark");
        if (!target || !(target instanceof HTMLElement)) return false;
        const id = Number(target.dataset.comment);
        const rects = Array.from(view.dom.querySelectorAll(`.comment-mark[data-comment="${id}"]`)).map((el) => el.getBoundingClientRect());
        const top = Math.min(...rects.map((r) => r.top));
        const bottom = Math.max(...rects.map((r) => r.bottom));
        onComment()(id, top, bottom);
        return false;
      },
    },
  });
}
