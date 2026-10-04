import assert from "node:assert/strict";
import { test } from "node:test";
import { EditorState, TextSelection } from "prosemirror-state";
import { type LinkMeta, type LinkResolver, insertLink, linkMenu, linkQuery, linksKey, linksPlugin } from "../src/links.ts";
import { parseMarkdown, serializeMarkdown } from "../src/markdown.ts";

const NOTES: Record<string, { title: string; kind: string }[]> = {
  "mara venn": [{ title: "Mara Venn", kind: "Character" }],
  mara: [{ title: "Mara Venn", kind: "Character" }],
  venn: [
    { title: "Mara Venn", kind: "Character" },
    { title: "Joss Venn", kind: "Character" },
  ],
};

function fakeResolver(): LinkResolver & { opened: [string, number][]; made: [string, string][] } {
  const resolver = {
    opened: [] as [string, number][],
    made: [] as [string, string][],
    resolve(target: string) {
      const notes = NOTES[target.toLowerCase()] ?? [];
      return { state: notes.length === 0 ? "loose" : notes.length === 1 ? "found" : "ambiguous", notes } as const;
    },
    suggest: () => [],
    open: (target: string, index: number) => void resolver.opened.push([target, index]),
    kinds: () => [
      ["character", "Character"],
      ["event", "Plot point"],
    ],
    create: (title: string, kind: string) => void resolver.made.push([title, kind]),
  };
  return resolver;
}

function load(markdown: string, resolver: LinkResolver | null = fakeResolver()): EditorState {
  return EditorState.create({ doc: parseMarkdown(markdown), plugins: [linksPlugin(() => resolver)] });
}

function marked(state: EditorState): string[] {
  return linksKey
    .getState(state)!
    .decorations.find()
    .map((d) => `${state.doc.nodeAt(d.from)!.attrs.target}: ${(d as unknown as { type: { attrs: { class: string } } }).type.attrs.class}`);
}

test("marks links with no note yet, and names two notes share", () => {
  const state = load("[[Mara Venn|her]] by [[The Drowning]], with a [[Venn]] and [[mara]].\n");
  assert.deepEqual(marked(state), ["The Drowning: loose", "Venn: ambiguous"]);
});

test("marks nothing until there's a resolver, then updates when told", () => {
  let state = load("[[The Drowning]]\n", null);
  assert.deepEqual(marked(state), []);
  const meta: LinkMeta = { resolver: fakeResolver() };
  state = state.apply(state.tr.setMeta(linksKey, meta));
  assert.deepEqual(marked(state), ["The Drowning: loose"]);
});

test("clicking offers to open, choose or make a note", () => {
  const resolver = fakeResolver();
  const labels = (target: string) => linkMenu(resolver, target, resolver.resolve(target)).map((i) => (i === "separator" ? "—" : i.label));
  assert.deepEqual(labels("Mara"), ["Open “Mara Venn”"]);
  assert.deepEqual(labels("Venn"), ["“Venn” could be:", "Mara Venn · Character", "Joss Venn · Character"]);
  assert.deepEqual(labels("The Drowning"), ["No note for “The Drowning” yet", "—", "New character", "New plot point"]);

  const [, , makeEvent] = linkMenu(resolver, "The Drowning", resolver.resolve("The Drowning"));
  assert.ok(makeEvent !== "separator");
  makeEvent.run?.();
  assert.deepEqual(resolver.made, [["The Drowning", "character"]]);
});

function typed(markdown: string): EditorState {
  const state = EditorState.create({ doc: parseMarkdown(markdown) });
  return state.apply(state.tr.setSelection(TextSelection.atEnd(state.doc)));
}

test("finds the link being typed before the cursor", () => {
  assert.deepEqual(linkQuery(typed("She met [[Ma")), { from: 9, query: "Ma" });
  assert.deepEqual(linkQuery(typed("She met [[")), { from: 9, query: "" });
  assert.equal(linkQuery(typed("She met [[Mara]] again")), null);
  assert.equal(linkQuery(typed("A [[Mara|her")), null, "a label is typed by hand");
  assert.equal(linkQuery(typed("Just text")), null);
});

test("a picked note becomes a link to its title, showing an alias if one was picked", () => {
  const state = typed("She met [[ma");
  const { from } = linkQuery(state)!;
  assert.equal(serializeMarkdown(state.apply(insertLink(state, from, "Mara Venn", "Mara")).doc), "She met [[Mara Venn|Mara]]\n");
  assert.equal(serializeMarkdown(state.apply(insertLink(state, from, "Mara Venn", null)).doc), "She met [[Mara Venn]]\n");
  assert.equal(serializeMarkdown(state.apply(insertLink(state, from, "Mara Venn", "Mara Venn")).doc), "She met [[Mara Venn]]\n");
});
