import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { test } from "node:test";
import type { Node } from "prosemirror-model";
import { countWords, parseMarkdown, serializeMarkdown } from "../src/markdown.ts";
import { schema } from "../src/schema.ts";

const fixtures = new URL("../../fixtures/", import.meta.url);
const read = (path: string) => readFileSync(new URL(path, fixtures), "utf8");
const list = (dir: string) => readdirSync(new URL(dir, fixtures)).sort();
const roundTrip = (markdown: string) => serializeMarkdown(parseMarkdown(markdown));

for (const name of list("markdown/canonical/")) {
  test(`canonical/${name} round-trips byte-identical`, () => {
    const markdown = read(`markdown/canonical/${name}`);
    assert.equal(roundTrip(markdown), markdown);
  });
}

for (const name of list("markdown/normalize/").filter((n) => n.endsWith(".in.md"))) {
  const base = name.slice(0, -".in.md".length);
  test(`normalize/${base} reaches canonical form and stays there`, () => {
    const once = roundTrip(read(`markdown/normalize/${name}`));
    assert.equal(once, read(`markdown/normalize/${base}.out.md`));
    assert.equal(roundTrip(once), once);
  });
}

const manuscript = "sample-vault/projects/tidewater/manuscript/";
for (const name of list(manuscript)) {
  test(`sample scene ${name} is already canonical`, () => {
    const body = read(manuscript + name).replace(/^---\n[\s\S]*?\n---\n\n/, "");
    assert.equal(roundTrip(body), body);
  });
}

const { nodes, marks } = schema;
const paragraph = (...content: Node[]) => nodes.doc.create(null, nodes.paragraph.create(null, content));

function assertSurvives(doc: Node) {
  const markdown = serializeMarkdown(doc);
  assert.ok(parseMarkdown(markdown).eq(doc), `changed after save and reload: ${JSON.stringify(markdown)}`);
}

test("typed text that looks like Markdown stays text", () => {
  const samples = [
    "[a](b)",
    "[[Mara]]",
    "[sic]",
    "a]b(c)",
    "5 * 3",
    "**not bold**",
    "_underscored_",
    "snake_case",
    "# not a heading",
    "#hashtag",
    "1. not a list",
    "1) not a list either",
    "- not a bullet",
    "+ plus",
    "> not a quote",
    "* * *",
    "C:\\Users",
    "tilde ~ and `ticks`",
    "<b>tags</b>",
    "<u>not underlined</u>",
    "a lone </u>",
    "Salt &amp; pepper",
    "---",
  ];
  for (const text of samples) assertSurvives(paragraph(schema.text(text)));
});

test("marks survive", () => {
  assertSurvives(
    paragraph(
      schema.text("plain "),
      schema.text("em", [marks.em.create()]),
      schema.text(" "),
      schema.text("strong", [marks.strong.create()]),
      schema.text(" "),
      schema.text("both", [marks.em.create(), marks.strong.create()]),
      schema.text(" "),
      schema.text("link", [marks.link.create({ href: "https://example.com/a b", title: "A \"title\"" })]),
      schema.text(" "),
      schema.text("under", [marks.underline.create()]),
      schema.text("lined", [marks.em.create(), marks.underline.create()]),
      schema.text(" "),
      schema.text("all", [marks.em.create(), marks.strong.create(), marks.underline.create()]),
    ),
  );
});

test("trailing spaces are not written", () => {
  const doc = nodes.doc.create(null, [
    nodes.paragraph.create(null, schema.text("Ends with a space. ")),
    nodes.paragraph.create(null, schema.text("Next.")),
  ]);
  assert.equal(serializeMarkdown(doc), "Ends with a space.\n\nNext.\n");
});

test("wikilinks survive, with and without labels", () => {
  assertSurvives(
    paragraph(
      nodes.wikilink.create({ target: "Mara Venn" }),
      schema.text("’s compass and "),
      nodes.wikilink.create({ target: "Old Teodor", label: "Teodor" }),
      schema.text("."),
    ),
  );
});

test("hard breaks and scene breaks survive", () => {
  assertSurvives(
    nodes.doc.create(null, [
      nodes.paragraph.create(null, [schema.text("one"), nodes.hard_break.create(), schema.text("two")]),
      nodes.horizontal_rule.create(),
      nodes.paragraph.create(null, schema.text("three")),
    ]),
  );
});

test("empty document serializes to an empty file", () => {
  assert.equal(roundTrip(""), "");
});

test("word count", () => {
  const doc = parseMarkdown("Mara’s ship—the *Gull*—sank. [[Old Teodor|Teo]] didn't care.\n");
  assert.equal(countWords(doc), 8);
});
