import assert from "node:assert/strict";
import { test } from "node:test";
import { EditorState, TextSelection } from "prosemirror-state";
import { parseMarkdown } from "../src/markdown.ts";
import { type SpellMeta, type Spellchecker, spellcheckKey, spellcheckPlugin } from "../src/spellcheck.ts";

const DICTIONARY = new Set(["the", "cat", "sat", "on", "mat", "met", "and", "a", "dog", "see", "here"]);

function fakeChecker(): Spellchecker & { calls: number } {
  const added = new Set<string>();
  const checker = {
    calls: 0,
    check(word: string) {
      checker.calls++;
      return DICTIONARY.has(word.toLowerCase()) || added.has(word);
    },
    suggest: () => [],
    add: (word: string) => void added.add(word),
  };
  return checker;
}

function load(markdown: string, checker: Spellchecker | null = fakeChecker()): EditorState {
  return EditorState.create({ doc: parseMarkdown(markdown), plugins: [spellcheckPlugin(() => checker)] });
}

function flagged(state: EditorState): string[] {
  return spellcheckKey
    .getState(state)!
    .decorations.find()
    .sort((a, b) => a.from - b.from)
    .map((d) => state.doc.textBetween(d.from, d.to));
}

const endOfDoc = (state: EditorState) => TextSelection.atEnd(state.doc);

test("flags every misspelling as soon as a document loads", () => {
  assert.deepEqual(flagged(load("Teh cat sat on teh mat.\n\nThe dgo sat.\n")), ["Teh", "teh", "dgo"]);
});

test("names from wikilinks count as spelled right, possessives too", () => {
  assert.deepEqual(flagged(load("[[Old Teodor|Teo]] met Teodor and Teo’s cat. Mara sat.\n")), ["Mara"]);
});

test("link URLs are not checked", () => {
  assert.deepEqual(flagged(load("See <https://exampel.com/qwzx> here.\n")), []);
});

test("the word being typed isn't flagged until the cursor leaves it", () => {
  let state = load("The cat sat.\n");
  state = state.apply(state.tr.setSelection(endOfDoc(state)));
  state = state.apply(state.tr.insertText(" Teh"));
  assert.deepEqual(flagged(state), []);
  state = state.apply(state.tr.insertText(" "));
  assert.deepEqual(flagged(state), ["Teh"]);
});

test("moving the cursor away flags the word it was in", () => {
  let state = load("The cat sat.\n");
  state = state.apply(state.tr.setSelection(endOfDoc(state)));
  state = state.apply(state.tr.insertText(" Teh"));
  state = state.apply(state.tr.setSelection(TextSelection.atStart(state.doc)));
  assert.deepEqual(flagged(state), ["Teh"]);
});

test("edits elsewhere keep existing underlines on the right words", () => {
  let state = load("The cat sat.\n\nA dgo sat.\n");
  state = state.apply(state.tr.insertText("Here ", 1));
  assert.deepEqual(flagged(state), ["dgo"]);
});

test("fixing a word removes its underline", () => {
  let state = load("A dgo sat.\n");
  const from = 3; // "dgo" starts after "A "
  const tr = state.tr.insertText("dog", from, from + 3);
  state = state.apply(tr.setSelection(TextSelection.atEnd(tr.doc)));
  assert.deepEqual(flagged(state), []);
});

test("adding a word to the dictionary rechecks the document", () => {
  const checker = fakeChecker();
  let state = load("Teodor sat. Teodor met a cat.\n", checker);
  assert.deepEqual(flagged(state), ["Teodor", "Teodor"]);
  checker.add("Teodor");
  const meta: SpellMeta = { recheck: true };
  state = state.apply(state.tr.setMeta(spellcheckKey, meta));
  assert.deepEqual(flagged(state), []);
});

test("a checker can be attached after loading, and removed", () => {
  let state = load("Teh cat.\n", null);
  assert.deepEqual(flagged(state), []);
  const attach: SpellMeta = { checker: fakeChecker() };
  state = state.apply(state.tr.setMeta(spellcheckKey, attach));
  assert.deepEqual(flagged(state), ["Teh"]);
  const detach: SpellMeta = { checker: null };
  state = state.apply(state.tr.setMeta(spellcheckKey, detach));
  assert.deepEqual(flagged(state), []);
});

test("repeated words are only checked once", () => {
  const checker = fakeChecker();
  load("the cat the cat the cat the cat\n", checker);
  assert.equal(checker.calls, 2);
});
