// What the phone's formatting bar does (DESIGN §9; Format 1 on the canvas's Phone writing page).
// With no keyboard shortcuts on a phone, each button is a command here, and the bar shows which
// are on where the cursor is. The bar itself is drawn by the phone app.

import { lift, setBlockType, toggleMark, wrapIn } from "prosemirror-commands";
import type { MarkType, NodeType } from "prosemirror-model";
import { type Command, type EditorState, TextSelection } from "prosemirror-state";
import { liftListItem, wrapInList } from "prosemirror-schema-list";
import { schema } from "./schema.ts";

const { nodes, marks } = schema;

export type Format = "bold" | "italic" | "heading" | "quote" | "list" | "link" | "sceneBreak";

/** Whether `type` is around the cursor, at any depth. */
function inside(state: EditorState, type: NodeType): boolean {
  const { $from } = state.selection;
  for (let depth = $from.depth; depth > 0; depth--) {
    if ($from.node(depth).type === type) return true;
  }
  return false;
}

function markOn(state: EditorState, type: MarkType): boolean {
  const { from, to, empty, $from } = state.selection;
  if (empty) return !!type.isInSet(state.storedMarks ?? $from.marks());
  return state.doc.rangeHasMark(from, to, type);
}

/** The formats on where the cursor is, for the bar to show as pressed. */
export function activeFormats(state: EditorState): Format[] {
  const on: Format[] = [];
  if (markOn(state, marks.strong)) on.push("bold");
  if (markOn(state, marks.em)) on.push("italic");
  if (state.selection.$from.parent.type === nodes.heading) on.push("heading");
  if (inside(state, nodes.blockquote)) on.push("quote");
  if (inside(state, nodes.bullet_list)) on.push("list");
  return on;
}

/** Heading on and off: a level-2 heading, since a scene's own title is the level above. */
const toggleHeading: Command = (state, dispatch) =>
  state.selection.$from.parent.type === nodes.heading
    ? setBlockType(nodes.paragraph)(state, dispatch)
    : setBlockType(nodes.heading, { level: 2 })(state, dispatch);

const toggleQuote: Command = (state, dispatch) =>
  inside(state, nodes.blockquote) ? lift(state, dispatch) : wrapIn(nodes.blockquote)(state, dispatch);

const toggleList: Command = (state, dispatch) =>
  inside(state, nodes.bullet_list) ? liftListItem(nodes.list_item)(state, dispatch) : wrapInList(nodes.bullet_list)(state, dispatch);

/** Selected words become a link to the note they name; with nothing selected, `[[` starts one,
 * so the bar can offer notes. */
const link: Command = (state, dispatch) => {
  const { from, to, empty } = state.selection;
  if (empty) {
    dispatch?.(state.tr.insertText("[[").scrollIntoView());
    return true;
  }
  const words = state.doc.textBetween(from, to, " ").trim();
  if (!words || words.includes("\n")) return false;
  dispatch?.(state.tr.replaceWith(from, to, nodes.wikilink.create({ target: words })).scrollIntoView());
  return true;
};

/** A scene break after the paragraph the cursor is in, with the cursor in a new one after it. */
const sceneBreak: Command = (state, dispatch) => {
  const { $from } = state.selection;
  if ($from.depth < 1) return false;
  const after = $from.after(1);
  if (dispatch) {
    const tr = state.tr.insert(after, [nodes.horizontal_rule.create(), nodes.paragraph.create()]);
    tr.setSelection(TextSelection.create(tr.doc, after + 2));
    dispatch(tr.scrollIntoView());
  }
  return true;
};

export const formatCommands: Record<Format, Command> = {
  bold: toggleMark(marks.strong),
  italic: toggleMark(marks.em),
  heading: toggleHeading,
  quote: toggleQuote,
  list: toggleList,
  link,
  sceneBreak,
};
