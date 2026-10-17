import assert from "node:assert/strict";
import { test } from "node:test";
import { EditorState, TextSelection } from "prosemirror-state";
import { parseMarkdown, serializeMarkdown } from "../src/markdown.ts";
import { type Format, activeFormats, formatCommands } from "../src/touch.ts";

/** Parses `markdown` with the cursor at `|`, or the words between two `|` selected. */
function stateAt(markdown: string): EditorState {
  const doc = parseMarkdown(markdown.replaceAll("|", "⁣"));
  const marks: number[] = [];
  doc.descendants((node, pos) => {
    if (!node.isText) return;
    for (let i = node.text!.indexOf("⁣"); i >= 0; i = node.text!.indexOf("⁣", i + 1)) marks.push(pos + i);
  });
  assert.ok(marks.length === 1 || marks.length === 2, "one | for a cursor or two for a selection");
  let tr = EditorState.create({ doc }).tr;
  for (const at of [...marks].reverse()) tr = tr.delete(at, at + 1);
  const [from, to = from + 1] = marks;
  return EditorState.create({ doc: tr.doc, selection: TextSelection.create(tr.doc, from, to - 1) });
}

/** Presses a bar button, then types `text`, and returns the Markdown and the state. */
function press(name: Format, markdown: string, text = ""): { markdown: string; state: EditorState } {
  let state = stateAt(markdown);
  assert.ok(
    formatCommands[name](state, (tr) => (state = state.apply(tr))),
    `${name} did not apply`,
  );
  if (text) state = state.apply(state.tr.insertText(text));
  return { markdown: serializeMarkdown(state.doc), state };
}

test("bold and italic go on the selected words and show as on", () => {
  const { markdown, state } = press("italic", "She kept |the compass| close.");
  assert.equal(markdown, "She kept *the compass* close.\n");
  assert.deepEqual(activeFormats(state), ["italic"]);
  assert.equal(press("bold", "She kept |the compass| close.").markdown, "She kept **the compass** close.\n");
});

test("with nothing selected, italic carries on into what's typed next", () => {
  assert.equal(press("italic", "She said |", "never").markdown, "She said *never*\n");
});

test("pressing italic again on italic words takes it off", () => {
  assert.equal(press("italic", "She kept *|the compass|* close.").markdown, "She kept the compass close.\n");
});

test("heading turns the paragraph into a heading and back", () => {
  const on = press("heading", "The quay|");
  assert.equal(on.markdown, "## The quay\n");
  assert.ok(activeFormats(on.state).includes("heading"));
  assert.equal(press("heading", "## The quay|").markdown, "The quay\n");
});

test("quote and list wrap the paragraph, and take it out again", () => {
  assert.equal(press("quote", "“You came,” he said.|").markdown, "> “You came,” he said.\n");
  assert.equal(press("quote", "> “You came,” he said.|").markdown, "“You came,” he said.\n");
  const list = press("list", "Tar|");
  assert.match(list.markdown, /^- Tar\n$/);
  assert.ok(activeFormats(list.state).includes("list"));
  assert.equal(press("list", "- Tar|").markdown, "Tar\n");
});

test("link with nothing selected starts one, so the bar can offer notes", () => {
  assert.equal(press("link", "She saw |", "Teo").state.doc.textContent, "She saw [[Teo");
});

test("link makes the selected words a link to the note they name", () => {
  assert.equal(press("link", "She saw |Old Teodor| there.").markdown, "She saw [[Old Teodor]] there.\n");
});

test("scene break goes after the paragraph, with the cursor in a new one below it", () => {
  assert.equal(press("sceneBreak", "The market opened at |dusk.", "Morning.").markdown, "The market opened at dusk.\n\n---\n\nMorning.\n");
});

test("plain text shows nothing on", () => {
  assert.deepEqual(activeFormats(stateAt("Plain |words.")), []);
});
