import assert from "node:assert/strict";
import { test } from "node:test";
import { EditorState } from "prosemirror-state";
import { type HighlightMeta, highlightKey, highlightPlugin, termPattern } from "../src/highlight.ts";
import { parseMarkdown } from "../src/markdown.ts";

function highlighted(markdown: string, terms: string[]): string[] {
  let state = EditorState.create({ doc: parseMarkdown(markdown), plugins: [highlightPlugin()] });
  const meta: HighlightMeta = { terms };
  state = state.apply(state.tr.setMeta(highlightKey, meta));
  return highlightKey
    .getState(state)!
    .decorations.find()
    .map((d) => state.doc.textBetween(d.from, d.to));
}

test("highlights words that start with a search term, ignoring case", () => {
  assert.deepEqual(highlighted("The Ledger, the ledgers, a pledge.\n", ["ledg"]), ["Ledger", "ledgers"]);
  assert.deepEqual(highlighted("Left port at dawn.\n", ["left port"]), ["Left", "port"]);
  assert.deepEqual(highlighted("Café crème\n", ["caf"]), ["Café"]);
});

test("nothing to highlight clears it", () => {
  assert.deepEqual(highlighted("The ledger.\n", []), []);
  assert.deepEqual(highlighted("The ledger.\n", ["  ", "(*)"]), []);
  assert.equal(termPattern([]), null);
});
