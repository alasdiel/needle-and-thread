import assert from "node:assert/strict";
import { test } from "node:test";
import { history, undo } from "prosemirror-history";
import { type Command, EditorState, TextSelection } from "prosemirror-state";
import { newSublistItem, shiftEnter, sinkItem } from "../src/keymap.ts";
import { parseMarkdown, serializeMarkdown } from "../src/markdown.ts";

/** Parses `markdown` with the cursor placed where `|` is. */
function stateAt(markdown: string): EditorState {
  const doc = parseMarkdown(markdown);
  let cursor = -1;
  doc.descendants((node, pos) => {
    const i = node.isText ? node.text!.indexOf("|") : -1;
    if (cursor < 0 && i >= 0) cursor = pos + i;
  });
  assert.ok(cursor >= 0, "no | cursor marker");
  const tr = EditorState.create({ doc }).tr.delete(cursor, cursor + 1);
  return EditorState.create({ doc: tr.doc, selection: TextSelection.create(tr.doc, cursor) });
}

/** Runs `command`, then types `text` at the cursor, and returns the resulting Markdown. */
function run(command: Command, markdown: string, text = ""): string {
  let state = stateAt(markdown);
  assert.ok(
    command(state, (tr) => (state = state.apply(tr))),
    "command did not apply",
  );
  if (text) state = state.apply(state.tr.insertText(text));
  return serializeMarkdown(state.doc);
}

test("Shift+Enter at the end of an item starts a sublist", () => {
  assert.equal(
    run(shiftEnter, "- Act one|\n- Act two\n", "Inciting incident"),
    "- Act one\n  - Inciting incident\n- Act two\n",
  );
});

test("Shift+Enter mid-item moves the rest into a sublist", () => {
  assert.equal(run(shiftEnter, "- Act |one\n"), "- Act\n  - one\n");
});

test("Shift+Enter works on the first item of a list", () => {
  assert.equal(run(shiftEnter, "1. First|\n2. Second\n", "Detail"), "1. First\n   1. Detail\n2. Second\n");
});

test("Shift+Enter on an empty item indents it", () => {
  assert.equal(run(shiftEnter, "- Act one\n- |\n", "Turn"), "- Act one\n  - Turn\n");
});

test("Shift+Enter in a sublist nests one level deeper", () => {
  assert.equal(
    run(shiftEnter, "- Act one\n  - Turn|\n", "Detail"),
    "- Act one\n  - Turn\n    - Detail\n",
  );
});

test("Shift+Enter outside a list is a line break", () => {
  assert.equal(run(shiftEnter, "Hello|world\n"), "Hello\\\nworld\n");
});

test("newSublistItem does nothing outside a list", () => {
  assert.equal(newSublistItem(stateAt("Plain| text\n")), false);
});

test("sinking an item into a tight list keeps it tight", () => {
  assert.equal(run(sinkItem, "- One\n- Two|\n"), "- One\n  - Two\n");
});

test("Shift+Enter is one transaction, undone in one step", () => {
  const start = stateAt("- Act one|\n");
  let state = EditorState.create({ doc: start.doc, selection: start.selection, plugins: [history()] });
  let dispatched = 0;
  shiftEnter(state, (tr) => {
    dispatched++;
    state = state.apply(tr);
  });
  assert.equal(dispatched, 1);
  assert.equal(serializeMarkdown(state.doc), "- Act one\n  -\n");
  undo(state, (tr) => (state = state.apply(tr)));
  assert.equal(serializeMarkdown(state.doc), "- Act one\n");
});
