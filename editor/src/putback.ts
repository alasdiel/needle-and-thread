// Putting a passage back from History. Rust works out the whole new text (core's
// `diff::put_back`); this turns it into one edit that touches only what differs, so the rest of
// the document, the undo history and the scroll position stay as they were.

import type { Node } from "prosemirror-model";
import { type EditorState, TextSelection, type Transaction } from "prosemirror-state";

/** The edit that turns the document into `doc`, with what changed selected and scrolled to, or
 * with `select` false, the cursor left where it was; null if nothing would change. */
export function changeTo(state: EditorState, doc: Node, select = true): Transaction | null {
  const start = state.doc.content.findDiffStart(doc.content);
  if (start === null) return null;
  let { a: endOld, b: endNew } = state.doc.content.findDiffEnd(doc.content)!;
  // Repeated text can make the two ends overlap the start; push them past it.
  const overlap = start - Math.min(endOld, endNew);
  if (overlap > 0) {
    endOld += overlap;
    endNew += overlap;
  }
  const tr = state.tr.replace(start, endOld, doc.slice(start, endNew));
  if (!select) return tr;
  const $start = tr.doc.resolve(start);
  const $end = tr.doc.resolve(tr.mapping.map(endOld, 1));
  return tr.setSelection(TextSelection.between($start, $end)).scrollIntoView();
}
