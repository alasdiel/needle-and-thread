// Cutting a passage to the cut bin, and putting it back. The bin's files are Rust's (the vault
// crate); this decides what a cut takes out, what it notes about the text around it, and where
// a passage goes when it comes back.
//
// A passage records the sentence before it and the sentence after it, and whether it began or
// ended a paragraph. Putting it back looks for that sentence pair in the text, so the spot can
// still be found after the scene has been saved, closed and edited elsewhere.

import { Fragment, type Node, type ResolvedPos, Slice } from "prosemirror-model";
import { type EditorState, TextSelection, type Transaction } from "prosemirror-state";
import { parseMarkdown, serializeMarkdown } from "./markdown.ts";
import { schema } from "./schema.ts";

/** A passage as the bin stores it. The names match the file's fields (and Rust's). */
export interface Passage {
  markdown: string;
  text_before: string;
  text_after: string;
  starts_paragraph: boolean;
  ends_paragraph: boolean;
  /** Whether it began (or ended) with a space inside its paragraph, which Markdown drops. */
  starts_with_space?: boolean;
  ends_with_space?: boolean;
}

/** How much of the text around a passage to keep, at most, in characters. */
const CONTEXT = 200;

// --- Text blocks -----------------------------------------------------------------------------

/** A paragraph, heading or other textblock, with its text and where each character is. */
interface Block {
  /** Where its content starts and ends. */
  start: number;
  end: number;
  text: string;
  /** The document position before each character, and one more for the end. A link's text
   * maps to the position before the link. */
  positions: number[];
}

function blocks(doc: Node): Block[] {
  const found: Block[] = [];
  doc.descendants((node, pos) => {
    if (!node.isTextblock) return true;
    const start = pos + 1;
    let text = "";
    const positions: number[] = [];
    node.forEach((child, offset) => {
      const at = start + offset;
      if (child.isText) {
        for (let i = 0; i < child.text!.length; i++) positions.push(at + i);
        text += child.text;
      } else {
        const leaf = child.type.spec.leafText?.(child) ?? "";
        for (let i = 0; i < leaf.length; i++) positions.push(at);
        text += leaf;
      }
    });
    positions.push(start + node.content.size);
    found.push({ start, end: start + node.content.size, text, positions });
    return false;
  });
  return found;
}

/** The index in `block.text` of document position `pos`. */
function indexOf(block: Block, pos: number): number {
  const i = block.positions.indexOf(pos);
  return i < 0 ? block.text.length : i;
}

// --- Sentences -------------------------------------------------------------------------------

/** A sentence's end: its punctuation, any closing quotes or brackets, and the space after. */
const SENTENCE_END = /[.!?…]+["'”’)\]]*\s+/gu;

/** The last sentence of `text`, with any space after it; long ones keep their last words. */
export function lastSentence(text: string): string {
  let start = 0;
  for (const m of text.matchAll(SENTENCE_END)) {
    const end = m.index + m[0].length;
    if (end < text.length) start = end;
  }
  let sentence = text.slice(start);
  if (sentence.length > CONTEXT) {
    sentence = sentence.slice(-CONTEXT);
    sentence = sentence.slice(sentence.search(/\s/u) + 1);
  }
  return sentence;
}

/** The first sentence of `text`, with any space before it; long ones keep their first words. */
export function firstSentence(text: string): string {
  let sentence = text;
  for (const m of text.matchAll(SENTENCE_END)) {
    const end = m.index + m[0].trimEnd().length;
    if (text.slice(0, end).trim()) {
      sentence = text.slice(0, end);
      break;
    }
  }
  if (sentence.length > CONTEXT) {
    sentence = sentence.slice(0, CONTEXT);
    sentence = sentence.slice(0, Math.max(sentence.trimEnd().lastIndexOf(" "), 1));
  }
  return sentence;
}

// --- Cutting ---------------------------------------------------------------------------------

/** The selection as a document of its own, to write as Markdown. Text from inside one block
 * becomes a paragraph; list items keep their list. */
function passageDoc(slice: Slice, $from: ResolvedPos, $to: ResolvedPos): Node {
  let content = slice.content;
  const first = content.firstChild;
  if (first?.isInline) {
    content = Fragment.from(schema.nodes.paragraph.create(null, content));
  } else if (first?.type === schema.nodes.list_item) {
    content = Fragment.from($from.node($from.sharedDepth($to.pos)).copy(content));
  }
  return schema.topNodeType.create(null, content);
}

/** What cutting the selection to the bin takes out, and the transaction that takes it out.
 * Whole paragraphs go entirely, so no empty line is left behind. Null for an empty selection. */
export function cutPassage(state: EditorState): { passage: Passage; tr: Transaction } | null {
  const { selection, doc } = state;
  if (selection.empty) return null;
  const { $from, $to } = selection;

  // A selection that starts or ends outside a paragraph (a scene break, or everything) takes
  // whole blocks there.
  const starts = !$from.parent.isTextblock || $from.parentOffset === 0;
  const ends = !$to.parent.isTextblock || $to.parentOffset === $to.parent.content.size;
  const list = blocks(doc);
  const within = (pos: number) => list.find((b) => b.start <= pos && pos <= b.end)!;
  const before = starts
    ? (list.filter((b) => b.end < $from.pos).pop()?.text ?? "")
    : within($from.pos).text.slice(0, indexOf(within($from.pos), $from.pos));
  const after = ends
    ? (list.find((b) => b.start > $to.pos)?.text ?? "")
    : within($to.pos).text.slice(indexOf(within($to.pos), $to.pos));

  // Whole paragraphs go entirely, with a list item they were all of.
  const tr = state.tr;
  let content = doc.slice($from.pos, $to.pos);
  let [$start, $end] = [$from, $to];
  if (starts && ends) {
    // Out to whole list items (or other blocks) when the selection covers them.
    const shared = Math.min($from.sharedDepth($to.pos), $from.depth - 1, $to.depth - 1);
    let fromDepth = $from.depth;
    while (fromDepth - 1 > shared && $from.index(fromDepth - 1) === 0) fromDepth--;
    let toDepth = $to.depth;
    while (toDepth - 1 > shared && $to.index(toDepth - 1) === $to.node(toDepth - 1).childCount - 1) toDepth--;
    tr.deleteRange(fromDepth ? $from.before(fromDepth) : $from.pos, toDepth ? $to.after(toDepth) : $to.pos);
    tr.mapping.maps[0]?.forEach((from, to) => {
      [$start, $end] = [doc.resolve(from), doc.resolve(to)];
      content = doc.slice(from, to);
    });
  } else {
    tr.deleteSelection();
  }
  const markdown = serializeMarkdown(passageDoc(content, $start, $end));
  if (!markdown.trim()) return null;
  const text = doc.textBetween($from.pos, $to.pos, "\n\n", (leaf) => leaf.type.spec.leafText?.(leaf) ?? "");
  return {
    passage: {
      markdown,
      text_before: lastSentence(before),
      text_after: firstSentence(after),
      starts_paragraph: starts,
      ends_paragraph: ends,
      ...(!starts && /^[^\S\n]/u.test(text) && { starts_with_space: true }),
      ...(!ends && /[^\S\n]$/u.test(text) && { ends_with_space: true }),
    },
    tr: tr.scrollIntoView(),
  };
}

// --- Putting back ----------------------------------------------------------------------------

/** Text with each run of white space made one space, and where each character came from. */
function collapse(text: string): { text: string; from: number[] } {
  let out = "";
  const from: number[] = [];
  for (let i = 0; i < text.length; i++) {
    if (/\s/u.test(text[i])) {
      if (i > 0 && /\s/u.test(text[i - 1])) continue;
      out += " ";
    } else {
      out += text[i];
    }
    from.push(i);
  }
  from.push(text.length);
  return { text: out, from };
}

const plain = (text: string) => collapse(text).text.trim();
const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** Where a passage goes back in, and the spaces lost from around it when the scene was saved
 * (a paragraph's Markdown drops spaces at its ends). */
interface Spot {
  pos: number;
  spaceBefore: boolean;
  spaceAfter: boolean;
  /** For whole paragraphs: they go after (or before) the textblock at `pos`. */
  edge?: "after" | "before";
}

/** The one place `passage` came from, found by the text that was around it; null if that text
 * isn't there any more, or is there more than once. */
export function findSpot(doc: Node, passage: Passage): Spot | null {
  const list = blocks(doc);
  const last = list.length - 1;
  const b = plain(passage.text_before);
  const a = plain(passage.text_after);
  const hadSpaceBefore = /\s$/u.test(passage.text_before);
  const hadSpaceAfter = /^\s/u.test(passage.text_after);
  const endsWith = (i: number) => plain(list[i].text).endsWith(b);
  const startsWith = (i: number) => plain(list[i].text).startsWith(a);
  // With no text before or after, the passage was at the very start or end.
  const prevOk = (i: number) => (b ? i > 0 && endsWith(i - 1) : i === 0);
  const nextOk = (i: number) => (a ? i < last && startsWith(i + 1) : i === last);
  const blockEnd = (i: number): Spot => ({
    pos: list[i].end,
    spaceBefore: hadSpaceBefore && !/\s$/u.test(list[i].text),
    spaceAfter: false,
  });
  const blockStart = (i: number): Spot => ({
    pos: list[i].start,
    spaceBefore: false,
    spaceAfter: hadSpaceAfter && !/^\s/u.test(list[i].text),
  });

  const spots: Spot[] = [];
  list.forEach((block, i) => {
    if (!passage.starts_paragraph && !passage.ends_paragraph) {
      // Cut from inside a paragraph, so the text either side is now joined.
      const { text, from } = collapse(block.text);
      const pattern = new RegExp(`${escape(b)}( ?)${escape(a)}`, "gu");
      for (const m of text.matchAll(pattern)) {
        const at = m.index + b.length + (hadSpaceBefore ? m[1].length : 0);
        const index = at > 0 ? from[at - 1] + 1 : 0;
        spots.push({
          pos: block.positions[index],
          spaceBefore: hadSpaceBefore && !/\s/u.test(block.text[index - 1] ?? ""),
          spaceAfter: hadSpaceAfter && !/\s/u.test(block.text[index] ?? ""),
        });
        if (!b && !a) break;
      }
    } else if (!passage.starts_paragraph) {
      if (b && endsWith(i) && nextOk(i)) spots.push(blockEnd(i));
    } else if (!passage.ends_paragraph) {
      if (a && startsWith(i) && prevOk(i)) spots.push(blockStart(i));
    } else if (b ? endsWith(i) && nextOk(i) : i === 0 && (a ? startsWith(i) : last === 0 && !block.text)) {
      spots.push(b ? { ...blockEnd(i), edge: "after" } : { ...blockStart(i), edge: "before" });
    }
  });
  return spots.length === 1 ? spots[0] : null;
}

/** How deep a slice of `content` is open on one side: down to its first (or last) textblock. */
function openDepth(content: Fragment, side: "first" | "last"): number {
  let depth = 0;
  for (let node = content[side === "first" ? "firstChild" : "lastChild"]; node; ) {
    depth++;
    if (node.isTextblock) return depth;
    node = side === "first" ? node.firstChild : node.lastChild;
  }
  return 0;
}

const isList = (node: Node | null) => node?.type.spec.content === "list_item+";

/** The passage as a slice that joins the text around it the way it did before it was cut. A
 * list item cut whole keeps its list open, so it goes back in among the other items. */
function passageSlice(passage: Passage): Slice {
  const { content } = parseMarkdown(passage.markdown);
  return new Slice(
    content,
    passage.starts_paragraph ? Number(isList(content.firstChild)) : openDepth(content, "first"),
    passage.ends_paragraph ? Number(isList(content.lastChild)) : openDepth(content, "last"),
  );
}

/** Puts a passage back where it came from, or else at the cursor (after the selection, if
 * there is one), and selects it. Says which it did. */
export function restorePassage(state: EditorState, passage: Passage): { tr: Transaction; atSpot: boolean } {
  const found = findSpot(state.doc, passage);
  const spot = found ?? { pos: state.selection.to, spaceBefore: false, spaceAfter: false };
  const tr = state.tr;
  const slice = passageSlice(passage);
  let pos = spot.pos;
  if (spot.edge) {
    // Beside the textblock, or for a list item, beside the item it's in.
    const $pos = tr.doc.resolve(pos);
    let depth = $pos.depth;
    for (let d = depth - 1; slice.openStart > 0 && d > 0; d--) {
      if ($pos.node(d).type === schema.nodes.list_item) {
        depth = d;
        break;
      }
    }
    pos = spot.edge === "after" ? $pos.after(depth) : $pos.before(depth);
  }
  // Spaces the passage had at its ends, and any the text around it lost.
  const after = " ".repeat(Number(spot.spaceAfter) + Number(!passage.ends_paragraph && !!passage.ends_with_space));
  const before = " ".repeat(Number(spot.spaceBefore) + Number(!passage.starts_paragraph && !!passage.starts_with_space));
  if (after) tr.insertText(after, pos);
  if (before) {
    tr.insertText(before, pos);
    pos += before.length;
  }
  const steps = tr.mapping.maps.length;
  tr.replaceRange(pos, pos, slice);
  const inserted = tr.mapping.slice(steps);
  const $start = tr.doc.resolve(inserted.map(pos, -1));
  const $end = tr.doc.resolve(inserted.map(pos, 1));
  return { tr: tr.setSelection(TextSelection.between($start, $end)).scrollIntoView(), atSpot: found !== null };
}

/** Whether `markdown` still has the place `passage` came from. */
export function hasSpot(markdown: string, passage: Passage): boolean {
  return findSpot(parseMarkdown(markdown), passage) !== null;
}
