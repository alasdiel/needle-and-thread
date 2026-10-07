import assert from "node:assert/strict";
import { test } from "node:test";
import { EditorState } from "prosemirror-state";
import { parseMarkdown, serializeMarkdown } from "../src/markdown.ts";
import { changeTo } from "../src/putback.ts";

/** Changes `before` to `after` and returns the result and the selected text. */
function change(before: string, after: string) {
  const state = EditorState.create({ doc: parseMarkdown(before) });
  const tr = changeTo(state, parseMarkdown(after));
  assert.ok(tr);
  const { from, to } = tr.selection;
  return { markdown: serializeMarkdown(tr.doc), selected: tr.doc.textBetween(from, to, "\n\n") };
}

test("a sentence put back mid-paragraph is selected", () => {
  const { markdown, selected } = change(
    "The smell came in. She kept the compass.\n",
    "The smell came in. A fiddler was tuning, badly. She kept the compass.\n",
  );
  assert.equal(markdown, "The smell came in. A fiddler was tuning, badly. She kept the compass.\n");
  assert.equal(selected.trim(), "A fiddler was tuning, badly.");
});

test("a paragraph put back between two others", () => {
  const { markdown, selected } = change("First.\n\nThird.\n", "First.\n\nSecond, *again*.\n\nThird.\n");
  assert.equal(markdown, "First.\n\nSecond, *again*.\n\nThird.\n");
  assert.equal(selected.trim(), "Second, again.");
});

test("only the changed part is replaced", () => {
  const state = EditorState.create({ doc: parseMarkdown("One.\n\nTwo.\n\nThree.\n") });
  const tr = changeTo(state, parseMarkdown("One.\n\nTwo, and more.\n\nThree.\n"));
  assert.ok(tr);
  assert.equal(tr.steps.length, 1);
  // "One." is untouched: it maps to itself.
  assert.equal(tr.mapping.map(2), 2);
});

test("repeated text around the change still lands in the right place", () => {
  const { markdown } = change("the the end\n", "the the the end\n");
  assert.equal(markdown, "the the the end\n");
});

test("no change, no edit", () => {
  const state = EditorState.create({ doc: parseMarkdown("Same.\n") });
  assert.equal(changeTo(state, parseMarkdown("Same.\n")), null);
});
