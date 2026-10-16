import assert from "node:assert/strict";
import { test } from "node:test";
import { parseMarkdown, serializeMarkdown } from "../src/markdown.ts";
import { type MergeBox, blockParagraphs, paragraphCount, placeBox } from "../src/merge.ts";

const box = (start: number, end: number): MergeBox => ({ id: 0, start, end, when: "", segments: [] });

test("paragraphs are counted as Rust counts them", () => {
  assert.equal(paragraphCount("One\nstill one.\n\n\n  \nTwo.\n"), 2);
  assert.equal(paragraphCount(""), 0);
});

test("each block's paragraphs add up to the whole document's", () => {
  for (const markdown of [
    "First.\n\nSecond.\n\nThird.\n",
    "# A heading\n\nText.\n\n- one\n- two\n\nAfter.\n",
    "- loose\n\n- list\n\nThen.\n",
    "> Quoted.\n>\n> Twice.\n\nAfter.\n",
    "```\ncode\n\nwith a gap\n```\n\nAfter.\n",
  ]) {
    const doc = parseMarkdown(markdown);
    const spans = blockParagraphs(doc);
    assert.equal(spans.length, doc.childCount, markdown);
    assert.equal(spans.at(-1)![1], paragraphCount(serializeMarkdown(doc)), markdown);
  }
});

test("a clash outlines its paragraphs' blocks and goes after them", () => {
  const doc = parseMarkdown("First.\n\nSecond.\n\nThird.\n");
  const spans = blockParagraphs(doc);
  const second = doc.child(0).nodeSize + doc.child(1).nodeSize;
  assert.deepEqual(placeBox(doc, spans, box(1, 2)), { blocks: [1], after: second });
  assert.deepEqual(placeBox(doc, spans, box(0, 3)), { blocks: [0, 1, 2], after: doc.content.size });
});

test("a clash where this side removed the text goes after the paragraph before", () => {
  const doc = parseMarkdown("First.\n\nThird.\n");
  const spans = blockParagraphs(doc);
  assert.deepEqual(placeBox(doc, spans, box(1, 1)), { blocks: [], after: doc.child(0).nodeSize });
  assert.deepEqual(placeBox(doc, spans, box(0, 0)), { blocks: [], after: 0 });
});
