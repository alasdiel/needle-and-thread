import { baseKeymap, chainCommands, exitCode, toggleMark } from "prosemirror-commands";
import { redo, undo } from "prosemirror-history";
import { undoInputRule } from "prosemirror-inputrules";
import { keymap } from "prosemirror-keymap";
import type { Node } from "prosemirror-model";
import { type Command, type EditorState, type Plugin, Selection, type Transaction } from "prosemirror-state";
import { liftListItem, sinkListItem, splitListItem } from "prosemirror-schema-list";
import { schema } from "./schema.ts";

const { nodes, marks } = schema;

const isList = (node: Node) => node.type === nodes.bullet_list || node.type === nodes.ordered_list;

// sinkListItem gives a newly created sublist default (loose) spacing, which would put blank
// lines into a tight list. Match the list it was nested into instead.
function inheritTightness(tr: Transaction): Transaction {
  const $pos = tr.selection.$from;
  for (let depth = $pos.depth; depth > 0; depth--) {
    const inner = $pos.node(depth);
    if (!isList(inner)) continue;
    if (inner.childCount !== 1) return tr; // sank into an existing sublist; leave it alone
    for (let outerDepth = depth - 1; outerDepth > 0; outerDepth--) {
      const outer = $pos.node(outerDepth);
      if (isList(outer)) {
        if (outer.attrs.tight !== inner.attrs.tight) {
          tr.setNodeMarkup($pos.before(depth), null, { ...inner.attrs, tight: outer.attrs.tight });
        }
        return tr;
      }
    }
    return tr;
  }
  return tr;
}

export const sinkItem: Command = (state, dispatch) =>
  sinkListItem(nodes.list_item)(state, dispatch && ((tr) => dispatch(inheritTightness(tr))));

/** In a list: starts a new item nested under the current one (an empty item just indents). */
export const newSublistItem: Command = (state, dispatch) => {
  const { $from } = state.selection;
  if ($from.depth < 2 || $from.node(-1).type !== nodes.list_item) return false;
  if ($from.parent.content.size === 0) return sinkItem(state, dispatch);

  let split: Transaction | undefined;
  if (!splitListItem(nodes.list_item)(state, (tr) => (split = tr)) || !split) return false;
  const splitTr: Transaction = split;
  const afterSplit: EditorState = state.apply(splitTr);
  return sinkItem(
    afterSplit,
    dispatch &&
      ((sinkTr) => {
        // Replay into one transaction so a single undo reverts the whole thing.
        for (const step of sinkTr.steps) splitTr.step(step);
        splitTr.setSelection(Selection.fromJSON(splitTr.doc, sinkTr.selection.toJSON()));
        dispatch(splitTr.scrollIntoView());
      }),
  );
};

const insertHardBreak: Command = chainCommands(exitCode, (state, dispatch) => {
  dispatch?.(state.tr.replaceSelectionWith(nodes.hard_break.create()).scrollIntoView());
  return true;
});

export const shiftEnter: Command = chainCommands(newSublistItem, insertHardBreak);

export function buildKeymaps(): Plugin[] {
  return [
    keymap({
      "Mod-z": undo,
      "Shift-Mod-z": redo,
      "Mod-y": redo,
      // Like Docs: backspace right after an automatic change (curly quote, em dash) undoes it.
      Backspace: undoInputRule,
      "Mod-b": toggleMark(marks.strong),
      "Mod-i": toggleMark(marks.em),
      "Mod-u": toggleMark(marks.underline),
      "Shift-Enter": shiftEnter,
      Enter: splitListItem(nodes.list_item),
      "Mod-[": liftListItem(nodes.list_item),
      "Mod-]": sinkItem,
    }),
    keymap(baseKeymap),
  ];
}
