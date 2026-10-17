import assert from "node:assert/strict";
import { test } from "node:test";
import { EditorState } from "prosemirror-state";
import { commentsKey, commentsPlugin, findMark, proseOf } from "../src/comments.ts";
import { parseMarkdown } from "../src/markdown.ts";

const SCENE = "The market opened at dusk, as it always had,\nand nobody asked why the lanterns were lit before the bells.\n\nMara walked the length of it twice, *counting* stalls, and once more. She was [[Mara Venn|counting]] again.\n";

test("the prose is folded the way Rust folds it", () => {
  const doc = parseMarkdown(SCENE);
  const prose = proseOf(doc).chars.join("");
  // Rust: core::anchor::prose, then every run of whitespace as one space.
  assert.equal(prose, "The market opened at dusk, as it always had, and nobody asked why the lanterns were lit before the bells. Mara walked the length of it twice, counting stalls, and once more. She was counting again. ");
});

test("a comment's words are found across a paragraph break and emphasis", () => {
  const doc = parseMarkdown(SCENE);
  const at = findMark(doc, { id: 1, quote: "the bells. Mara walked", n: 0 });
  assert.ok(at);
  assert.equal(doc.textBetween(at.from, at.to, " "), "the bells. Mara walked");
  const stalls = findMark(doc, { id: 2, quote: "twice, counting stalls", n: 0 });
  assert.ok(stalls);
  assert.equal(doc.textBetween(stalls.from, stalls.to), "twice, counting stalls");
});

test("the nth occurrence is the one marked, a wikilink counting as its label", () => {
  const doc = parseMarkdown(SCENE);
  const second = findMark(doc, { id: 3, quote: "counting", n: 1 });
  assert.ok(second);
  assert.equal(doc.nodeAt(second.from)?.type.name, "wikilink");
  assert.equal(findMark(doc, { id: 4, quote: "counting", n: 2 }), null);
  assert.equal(findMark(doc, { id: 5, quote: "not in the text", n: 0 }), null);
});

test("marks are drawn, and the open one stronger", () => {
  const plugin = commentsPlugin(() => () => {});
  let state = EditorState.create({ doc: parseMarkdown(SCENE), plugins: [plugin] });
  state = state.apply(state.tr.setMeta(commentsKey, { marks: [{ id: 7, quote: "lit before the bells", n: 0 }] }));
  const found = commentsKey.getState(state)!.decorations.find();
  assert.equal(found.length, 1);
  assert.equal(state.doc.textBetween(found[0].from, found[0].to), "lit before the bells");
  state = state.apply(state.tr.setMeta(commentsKey, { active: 7 }));
  const active = commentsKey.getState(state)!.decorations.find()[0] as unknown as { type: { attrs: { class: string } } };
  assert.equal(active.type.attrs.class, "comment-mark active");
});
