// Where a paragraph was changed both here and on another device (DESIGN §5, Sync). Rust works
// out the clashes (core's `merge::clashes`) by paragraph: runs of lines between blank lines in
// the Markdown. This finds those paragraphs among the document's blocks, outlines them, and
// draws the other device's version under them with what differs marked and three ways to settle
// it. Decorations only; settling is the app's, which changes the text with `applyMarkdown`.

import type { Node } from "prosemirror-model";
import { Plugin, PluginKey } from "prosemirror-state";
import { Decoration, DecorationSet } from "prosemirror-view";
import { serializeMarkdown } from "./markdown.ts";

export type Choice = "theirs" | "both" | "mine";

export interface Segment {
  kind: "same" | "added" | "removed";
  text: string;
}

/** One clash: paragraphs `start` to `end` (not included) of the document are this side's, and
 * `segments` turn them into the other device's version. `when` says whose and when. */
export interface MergeBox {
  id: number;
  start: number;
  end: number;
  when: string;
  segments: Segment[];
}

export interface MergeMeta {
  boxes: MergeBox[];
}

interface MergeState {
  boxes: MergeBox[];
  decorations: DecorationSet;
}

export const mergeKey = new PluginKey<MergeState>("merge");

/** How many paragraphs a Markdown text has, counted as Rust counts them. */
export function paragraphCount(markdown: string): number {
  let count = 0;
  let inside = false;
  for (const line of markdown.split("\n")) {
    const blank = line.trim() === "";
    if (!blank && !inside) count++;
    inside = !blank;
  }
  return count;
}

/** For each top-level block, the first paragraph in it and the one after its last. */
export function blockParagraphs(doc: Node): [number, number][] {
  const spans: [number, number][] = [];
  let at = 0;
  doc.forEach((block) => {
    const count = paragraphCount(serializeMarkdown(doc.type.create(null, block)));
    spans.push([at, at + count]);
    at += count;
  });
  return spans;
}

/** Where a box goes: the blocks holding its paragraphs, and the position after them (after the
 * paragraph before, when this side removed them). */
export function placeBox(doc: Node, spans: [number, number][], box: MergeBox): { blocks: number[]; after: number } {
  const blocks: number[] = [];
  let after = 0;
  let pos = 0;
  spans.forEach(([from, to], index) => {
    const end = pos + doc.child(index).nodeSize;
    if (from < box.end && to > box.start) blocks.push(index);
    if (box.end > box.start ? from < box.end : from < box.start) after = end;
    pos = end;
  });
  return { blocks, after };
}

function decorate(doc: Node, boxes: MergeBox[], onChoose: (id: number, choice: Choice) => void): DecorationSet {
  if (boxes.length === 0) return DecorationSet.empty;
  const spans = blockParagraphs(doc);
  const decorations: Decoration[] = [];
  for (const box of boxes) {
    const { blocks, after } = placeBox(doc, spans, box);
    doc.forEach((block, offset, index) => {
      if (blocks.includes(index)) decorations.push(Decoration.node(offset, offset + block.nodeSize, { class: "merge-mine" }));
    });
    decorations.push(
      Decoration.widget(after, () => drawBox(box, onChoose), {
        side: 1,
        key: `merge-${box.id}-${box.start}-${JSON.stringify(box.segments)}`,
        stopEvent: () => true,
        ignoreSelection: true,
      }),
    );
  }
  return DecorationSet.create(doc, decorations);
}

/** The other device's version, under this side's. */
function drawBox(box: MergeBox, onChoose: (id: number, choice: Choice) => void): HTMLElement {
  const el = document.createElement("div");
  el.className = "merge-other";
  el.contentEditable = "false";
  el.dataset.merge = String(box.id);

  const when = document.createElement("div");
  when.className = "merge-when";
  when.textContent = box.when;
  el.append(when);

  const text = document.createElement("div");
  text.className = "merge-text";
  let paragraph = document.createElement("p");
  text.append(paragraph);
  for (const segment of box.segments) {
    const pieces = segment.text.split(/\n\s*\n/);
    pieces.forEach((piece, i) => {
      if (i > 0) {
        paragraph = document.createElement("p");
        text.append(paragraph);
      }
      if (!piece) return;
      const tag = segment.kind === "added" ? "ins" : segment.kind === "removed" ? "del" : "span";
      const span = document.createElement(tag);
      span.textContent = piece;
      paragraph.append(span);
    });
  }
  el.append(text);

  const actions = document.createElement("div");
  actions.className = "merge-actions";
  for (const [choice, label] of [
    ["theirs", "Use this one"],
    ["both", "Keep both"],
    ["mine", "Keep mine"],
  ] as const) {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = label;
    button.dataset.choice = choice;
    if (choice === "theirs") button.className = "primary";
    button.addEventListener("mousedown", (event) => event.preventDefault());
    button.addEventListener("click", () => onChoose(box.id, choice));
    actions.append(button);
  }
  el.append(actions);
  return el;
}

export function mergePlugin(onChoose: () => (id: number, choice: Choice) => void): Plugin<MergeState> {
  const choose = (id: number, choice: Choice) => onChoose()(id, choice);
  return new Plugin<MergeState>({
    key: mergeKey,
    state: {
      init: () => ({ boxes: [], decorations: DecorationSet.empty }),
      apply(tr, state) {
        const meta = tr.getMeta(mergeKey) as MergeMeta | undefined;
        if (meta) return { boxes: meta.boxes, decorations: decorate(tr.doc, meta.boxes, choose) };
        // They move with the text until the app works the clashes out again from what's typed.
        if (!tr.docChanged) return state;
        return { boxes: state.boxes, decorations: state.decorations.map(tr.mapping, tr.doc) };
      },
    },
    props: {
      decorations: (state) => mergeKey.getState(state)?.decorations,
    },
  });
}
