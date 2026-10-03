import assert from "node:assert/strict";
import { test } from "node:test";
import { parseMarkdown, serializeMarkdown } from "../src/markdown.ts";

// Mirrors Editor.splitParts (which needs a DOM to construct), splitting where `|` is. The
// marker's position is the same with or without it, since nothing before it changes.
function split(markdown: string, marker = "|") {
  let at = -1;
  parseMarkdown(markdown).descendants((node, pos) => {
    const i = node.isText ? node.text!.indexOf(marker) : -1;
    if (at < 0 && i >= 0) at = pos + i;
  });
  const withoutMarker = parseMarkdown(markdown.replace(marker, ""));
  return {
    before: serializeMarkdown(withoutMarker.cut(0, at)),
    after: serializeMarkdown(withoutMarker.cut(at)),
  };
}

test("splitting between paragraphs", () => {
  assert.deepEqual(split("One.\n\n|Two.\n"), { before: "One.\n", after: "Two.\n" });
});

test("splitting inside a paragraph splits the paragraph", () => {
  assert.deepEqual(split("The market |opened.\n"), { before: "The market\n", after: "opened.\n" });
});

test("splitting keeps formatting on both sides", () => {
  assert.deepEqual(split("*Before* and **af|ter**.\n"), { before: "*Before* and **af**\n", after: "**ter**.\n" });
});
