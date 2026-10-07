import assert from "node:assert/strict";
import { test } from "node:test";
import type { Node } from "prosemirror-model";
import { EditorState, TextSelection } from "prosemirror-state";
import { closeHistory, history, redo, undo } from "prosemirror-history";
import { type Move, type Passage, cutPassage, firstSentence, followHistory, lastSentence, restorePassage } from "../src/bin.ts";
import { parseMarkdown, serializeMarkdown } from "../src/markdown.ts";

/** Where `text` starts (or with `end`, ends) in the document. */
function at(doc: Node, text: string, end = false): number {
  let found = -1;
  doc.descendants((node, pos) => {
    const i = node.isText ? node.text!.indexOf(text) : -1;
    if (found < 0 && i >= 0) found = pos + i + (end ? text.length : 0);
  });
  assert.ok(found >= 0, `no “${text}”`);
  return found;
}

/** Selects from the start of `from` to the end of `to`, and cuts that to the bin. */
function cut(markdown: string, from: string, to: string) {
  const doc = parseMarkdown(markdown);
  const state = EditorState.create({ doc, selection: TextSelection.create(doc, at(doc, from), at(doc, to, true)) });
  const result = cutPassage(state);
  assert.ok(result);
  return { passage: result.passage, state: state.apply(result.tr) };
}

/** Puts the passage back, as soon as it's cut and again after a save and reopen. */
function putBack(state: EditorState, passage: Passage, original: string) {
  const now = restorePassage(state, passage);
  assert.ok(now.atSpot, "not found right after cutting");
  assert.equal(serializeMarkdown(now.tr.doc), original);
  const reopened = EditorState.create({ doc: parseMarkdown(serializeMarkdown(state.doc)) });
  const later = restorePassage(reopened, passage);
  assert.ok(later.atSpot, "not found after reopening");
  assert.equal(serializeMarkdown(later.tr.doc), original);
  return now.tr;
}

const MARKET =
  "The market opened at dusk. Mara walked the length of it twice. The smell of the harbor came in under everything. She kept the compass.\n\nOld Teodor was where the letter said.\n\nHe had not pointed true in forty years.\n";

test("sentences either side of a passage", () => {
  assert.equal(lastSentence("One. Two words. "), "Two words. ");
  assert.equal(lastSentence("One. Two words"), "Two words");
  assert.equal(lastSentence("“Here,” she said. "), "“Here,” she said. ");
  assert.equal(firstSentence(" She kept it. Then left."), " She kept it.");
  assert.equal(firstSentence("and then she left"), "and then she left");
  assert.equal(firstSentence("“Go.” He went."), "“Go.”");
});

test("a sentence cut from inside a paragraph", () => {
  const { passage, state } = cut(MARKET, "The smell", "everything.");
  assert.deepEqual(passage, {
    markdown: "The smell of the harbor came in under everything.\n",
    text_before: "Mara walked the length of it twice. ",
    text_after: " She kept the compass.",
    starts_paragraph: false,
    ends_paragraph: false,
  });
  assert.equal(
    serializeMarkdown(state.doc),
    MARKET.replace("The smell of the harbor came in under everything.", ""),
  );
  const tr = putBack(state, passage, MARKET);
  // What came back is selected.
  assert.equal(tr.doc.textBetween(tr.selection.from, tr.selection.to), "The smell of the harbor came in under everything.");
});

test("whole paragraphs go, leaving no empty line", () => {
  const { passage, state } = cut(MARKET, "Old Teodor", "the letter said.");
  assert.equal(passage.markdown, "Old Teodor was where the letter said.\n");
  assert.equal(passage.text_before, "She kept the compass.");
  assert.equal(passage.text_after, "He had not pointed true in forty years.");
  assert.ok(passage.starts_paragraph && passage.ends_paragraph);
  assert.equal(state.doc.childCount, 2);
  putBack(state, passage, MARKET);
});

test("from inside one paragraph to the end of the next", () => {
  const { passage, state } = cut(MARKET, "She kept", "the letter said.");
  assert.equal(passage.markdown, "She kept the compass.\n\nOld Teodor was where the letter said.\n");
  assert.ok(!passage.starts_paragraph && passage.ends_paragraph);
  putBack(state, passage, MARKET);
});

test("from the start of one paragraph into the next", () => {
  const { passage, state } = cut(MARKET, "Old Teodor", "not pointed");
  assert.ok(passage.starts_paragraph && !passage.ends_paragraph);
  assert.equal(passage.text_after, " true in forty years.");
  putBack(state, passage, MARKET);
});

test("across paragraphs, from inside one to inside another", () => {
  const { passage, state } = cut(MARKET, "everything.", "Old Teodor");
  assert.equal(serializeMarkdown(state.doc).split("\n\n").length, 2);
  putBack(state, passage, MARKET);
});

test("the first paragraph, and the last", () => {
  const first = cut(MARKET, "The market", "the compass.");
  assert.equal(first.passage.text_before, "");
  putBack(first.state, first.passage, MARKET);
  const last = cut(MARKET, "He had", "forty years.");
  assert.equal(last.passage.text_after, "");
  putBack(last.state, last.passage, MARKET);
});

test("links and formatting come back as they were", () => {
  const text = "Mara met [[Old Teodor|Teodor]] at the *market*, as **arranged**. Then she left.\n";
  const { passage, state } = cut(text, "at the", "arranged");
  assert.equal(passage.text_before, "Mara met Teodor ");
  putBack(state, passage, text);
});

test("a passage whose place has gone goes in at the cursor", () => {
  const { passage, state } = cut(MARKET, "The smell", "everything.");
  const edited = parseMarkdown(serializeMarkdown(state.doc).replace("She kept", "She hid"));
  const end = at(edited, "forty years.", true);
  const result = restorePassage(EditorState.create({ doc: edited, selection: TextSelection.create(edited, end) }), passage);
  assert.ok(!result.atSpot);
  assert.ok(serializeMarkdown(result.tr.doc).includes("forty years.The smell of the harbor came in under everything."));
});

test("a place that's there twice isn't guessed at", () => {
  const twice = "One. Two. Three.\n\nOne. Two. Three.\n";
  const { passage, state } = cut(twice, "Two.", "Two.");
  assert.equal(restorePassage(state, passage).atSpot, true);
  const both = EditorState.create({ doc: parseMarkdown("One.  Three.\n\nOne.  Three.\n") });
  assert.equal(restorePassage(both, passage).atSpot, false);
});

test("a list item cut whole comes back as an item", () => {
  const list = "Before.\n\n- one\n- two\n- three\n\nAfter.\n";
  const { passage, state } = cut(list, "two", "two");
  assert.equal(serializeMarkdown(state.doc), "Before.\n\n- one\n- three\n\nAfter.\n");
  putBack(state, passage, list);
});

test("several list items, and words from inside one", () => {
  const list = "Before.\n\n- one\n- two\n- three\n- four\n\nAfter.\n";
  const items = cut(list, "two", "three");
  assert.equal(serializeMarkdown(items.state.doc), "Before.\n\n- one\n- four\n\nAfter.\n");
  putBack(items.state, items.passage, list);
  const words = cut("- the first item\n- the second item\n", "second", "second");
  assert.equal(words.passage.markdown, "second\n");
  putBack(words.state, words.passage, "- the first item\n- the second item\n");
});

test("a heading and the paragraph under it", () => {
  const text = "Intro.\n\n## The harbor\n\nSalt and tar.\n\nThe end.\n";
  const { passage, state } = cut(text, "The harbor", "Salt and tar.");
  assert.equal(passage.markdown, "## The harbor\n\nSalt and tar.\n");
  assert.equal(serializeMarkdown(state.doc), "Intro.\n\nThe end.\n");
  putBack(state, passage, text);
});

test("everything at once", () => {
  const doc = parseMarkdown(MARKET);
  const state = EditorState.create({ doc, selection: TextSelection.create(doc, 0, doc.content.size) });
  const result = cutPassage(state)!;
  assert.equal(result.passage.markdown, MARKET);
  const empty = state.apply(result.tr);
  assert.equal(serializeMarkdown(empty.doc), "");
  putBack(empty, result.passage, MARKET);
});

test("spaces at a passage's ends come back with it", () => {
  const text = "She kept the compass wrapped in her sleeve. It was the last thing.\n";
  const leading = cut(text, " kept", "slee");
  assert.equal(leading.passage.starts_with_space, true);
  assert.equal(leading.passage.ends_with_space, undefined);
  assert.equal(serializeMarkdown(leading.state.doc), "Sheve. It was the last thing.\n");
  putBack(leading.state, leading.passage, text);
  const trailing = cut(text, "She kept", "sleeve. ");
  assert.equal(trailing.passage.ends_with_space, true);
  putBack(trailing.state, trailing.passage, text);
});

// --- Undo and the bin ----------------------------------------------------------------------

/** A state with undo history, the text from `from` to `to` selected. */
function withHistory(markdown: string, from: string, to: string): EditorState {
  const doc = parseMarkdown(markdown);
  return EditorState.create({ doc, plugins: [history()], selection: TextSelection.create(doc, at(doc, from), at(doc, to, true)) });
}

/** Runs Undo or Redo, as the editor's keymap would. */
function step(state: EditorState, command: typeof undo): EditorState {
  let next = state;
  assert.ok(command(state, (tr) => (next = state.apply(tr))));
  return next;
}

test("Undo after a cut brings the passage back out of the bin, and Redo puts it in again", () => {
  let state = withHistory(MARKET, "The smell", "everything.");
  const cut = cutPassage(state)!;
  const before = state.doc;
  state = state.apply(closeHistory(cut.tr));
  const moves: Move[] = [{ id: 1, passage: cut.passage, with: before, without: state.doc, inBin: true }];

  state = step(state, undo);
  assert.deepEqual(followHistory(moves, state.doc).back.map((m) => m.id), [1]);
  assert.equal(moves[0].inBin, false);

  state = step(state, redo);
  assert.deepEqual(followHistory(moves, state.doc).out.map((m) => m.id), [1]);
  assert.equal(moves[0].inBin, true);
});

test("Undo after a restore takes the passage out again", () => {
  const { passage, state: cutState } = cut(MARKET, "The smell", "everything.");
  let state = EditorState.create({ doc: cutState.doc, plugins: [history()] });
  const without = state.doc;
  const restored = restorePassage(state, passage);
  state = state.apply(closeHistory(restored.tr));
  const moves: Move[] = [{ id: 2, passage, with: state.doc, without, inBin: false }];

  state = step(state, undo);
  assert.deepEqual(followHistory(moves, state.doc).out.map((m) => m.id), [2]);
});

test("Undo that stops short of the cut leaves the bin alone", () => {
  let state = withHistory(MARKET, "The smell", "everything.");
  const cut = cutPassage(state)!;
  const before = state.doc;
  state = state.apply(closeHistory(cut.tr));
  const moves: Move[] = [{ id: 3, passage: cut.passage, with: before, without: state.doc, inBin: true }];
  // Typing after the cut, as its own step.
  state = state.apply(closeHistory(state.tr.insertText("New words. ", 1)));

  state = step(state, undo);
  assert.deepEqual(followHistory(moves, state.doc), { back: [], out: [] });
  assert.equal(moves[0].inBin, true);
});
