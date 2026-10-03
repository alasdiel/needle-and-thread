import assert from "node:assert/strict";
import { test } from "node:test";
import { parseMarkdown } from "../src/markdown.ts";
import { blockCount, cutLinePosition, seamPosition } from "../src/pattern.ts";

// Where the cut line goes for a cursor at `|`: between blocks it's shown as `---` on its own
// line, inside a paragraph as `|` in that paragraph's text. As in split.test.ts, the cursor's
// position is the same with or without the marker.
function cutLineAt(markdown: string): string {
  let cursor = -1;
  parseMarkdown(markdown).descendants((node, pos) => {
    const i = node.isText ? node.text!.indexOf("|") : -1;
    if (cursor < 0 && i >= 0) cursor = pos + i;
  });
  const doc = parseMarkdown(markdown.replace("|", ""));
  const $at = doc.resolve(cutLinePosition(doc.resolve(cursor)));
  if ($at.depth === 0) return `${$at.nodeBefore?.textContent ?? ""}\n---\n${$at.nodeAfter?.textContent ?? ""}`;
  const text = $at.parent.textContent;
  return `${text.slice(0, $at.parentOffset)}|${text.slice($at.parentOffset)}`;
}

// Merging joins the two scenes' Markdown with a blank line (needle-vault's merge_with_next), so
// the seam goes before the first block that came from the second scene.
function seamBefore(first: string, second: string): string | undefined {
  const index = blockCount(parseMarkdown(first));
  const merged = parseMarkdown(first.trim() === "" ? second : `${first.trimEnd()}\n\n${second}`);
  return merged.resolve(seamPosition(merged, index)).nodeAfter?.textContent;
}

test("an empty document has no blocks", () => {
  assert.equal(blockCount(parseMarkdown("")), 0);
});

test("blocks are counted at the top level only", () => {
  assert.equal(blockCount(parseMarkdown("One.\n\n- a\n- b\n\nTwo.\n")), 3);
});

test("the seam goes before the merged-in scene's first block", () => {
  assert.equal(seamBefore("One.\n\nTwo.\n", "Three.\n\nFour.\n"), "Three.");
});

test("merging into an empty scene puts the seam at the top", () => {
  assert.equal(seamBefore("", "Three.\n"), "Three.");
});

test("at the end of a paragraph the cut line goes between blocks", () => {
  assert.equal(cutLineAt("One.|\n\nTwo.\n"), "One.\n---\nTwo.");
});

test("at the start of a paragraph the cut line goes between blocks", () => {
  assert.equal(cutLineAt("One.\n\n|Two.\n"), "One.\n---\nTwo.");
});

test("inside a paragraph the cut line stays at the cursor", () => {
  assert.equal(cutLineAt("The market |opened.\n"), "The market |opened.");
});

test("inside a list the cut line stays at the cursor", () => {
  assert.equal(cutLineAt("- one|\n- two\n"), "one|");
});

test("a seam past the last block goes at the end", () => {
  const doc = parseMarkdown("One.\n");
  assert.equal(seamPosition(doc, 5), doc.content.size);
});
