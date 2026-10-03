// Markdown ⇄ ProseMirror document. DOM-free so it runs under `node --test`.
//
// The goal is a stable canonical form: serialize(parse(file)) === file for any file the app
// wrote, so saving never touches text the writer didn't change.

import MarkdownIt from "markdown-it";
import type Token from "markdown-it/lib/token.mjs";
import {
  MarkdownParser,
  MarkdownSerializer,
  MarkdownSerializerState,
  defaultMarkdownSerializer,
} from "prosemirror-markdown";
import type { Mark, Node } from "prosemirror-model";
import { schema } from "./schema.ts";

const md = new MarkdownIt("commonmark", { html: false });
// Rules for things the schema can't hold. With `code` off, indented paragraphs stay paragraphs.
md.disable(["code", "fence", "html_block", "html_inline", "backticks", "image"]);
md.use(wikilinks);
md.use(underline);
// markdown-it percent-encodes URLs while parsing, which would rewrite every non-ASCII or
// space-containing URL in a file on its first save. Keep them exactly as written.
// (`validateLink` still rejects javascript: and similar.)
md.normalizeLink = (url) => url;
md.normalizeLinkText = (text) => text;

function wikilinks(md: MarkdownIt): void {
  md.inline.ruler.before("link", "wikilink", (state, silent) => {
    const start = state.pos;
    if (state.src.charCodeAt(start) !== 0x5b || state.src.charCodeAt(start + 1) !== 0x5b) return false;
    const end = state.src.indexOf("]]", start + 2);
    if (end < 0) return false;
    const inner = state.src.slice(start + 2, end);
    if (/[[\]\n]/.test(inner)) return false;

    const bar = inner.indexOf("|");
    const target = (bar < 0 ? inner : inner.slice(0, bar)).trim();
    const label = bar < 0 ? "" : inner.slice(bar + 1).trim();
    if (!target) return false;

    if (!silent) {
      const token = state.push("wikilink", "", 0);
      token.attrSet("target", target);
      if (label) token.attrSet("label", label);
    }
    state.pos = end + 2;
    return true;
  });
}

// `<u>…</u>` is the only HTML recognized; html_inline stays off so other tags remain text.
function underline(md: MarkdownIt): void {
  md.inline.ruler.before("link", "underline", (state, silent) => {
    const start = state.pos;
    if (!state.src.startsWith("<u>", start)) return false;
    const end = state.src.indexOf("</u>", start + 3);
    if (end <= start + 3 || end + 4 > state.posMax) return false;

    if (!silent) {
      // Same approach as markdown-it's link rule: tokenize the inner range in place, so
      // emphasis inside the underline still works.
      const posMax = state.posMax;
      state.pos = start + 3;
      state.posMax = end;
      state.push("underline_open", "u", 1);
      state.md.inline.tokenize(state);
      state.push("underline_close", "u", -1);
      state.posMax = posMax;
    }
    state.pos = end + 4;
    return true;
  });
}

function listIsTight(tokens: readonly Token[], i: number): boolean {
  while (++i < tokens.length) {
    if (tokens[i].type !== "list_item_open") return tokens[i].hidden;
  }
  return false;
}

const parser = new MarkdownParser(schema, md, {
  blockquote: { block: "blockquote" },
  paragraph: { block: "paragraph" },
  list_item: { block: "list_item" },
  bullet_list: { block: "bullet_list", getAttrs: (_tok, tokens, i) => ({ tight: listIsTight(tokens, i) }) },
  ordered_list: {
    block: "ordered_list",
    getAttrs: (tok, tokens, i) => ({ order: Number(tok.attrGet("start") ?? 1), tight: listIsTight(tokens, i) }),
  },
  heading: { block: "heading", getAttrs: (tok) => ({ level: Number(tok.tag.slice(1)) }) },
  hr: { node: "horizontal_rule" },
  hardbreak: { node: "hard_break" },
  em: { mark: "em" },
  strong: { mark: "strong" },
  underline: { mark: "underline" },
  link: {
    mark: "link",
    getAttrs: (tok) => ({ href: tok.attrGet("href"), title: tok.attrGet("title") || null }),
  },
  wikilink: {
    node: "wikilink",
    noCloseToken: true,
    getAttrs: (tok) => ({ target: tok.attrGet("target"), label: tok.attrGet("label") }),
  },
});

const defaults = defaultMarkdownSerializer;
const defaultLink = defaults.marks.link as {
  open: (state: MarkdownSerializerState, mark: Mark, parent: Node, index: number) => string;
  close: (state: MarkdownSerializerState, mark: Mark, parent: Node, index: number) => string;
};

// The stock link serializer writes the URL bare, so one containing a space stops being a link
// on reload. CommonMark allows such URLs inside `<…>`.
const needsAngleBrackets = (href: string) => /[\s<>]/.test(href);
type LinkState = MarkdownSerializerState & { inAutolink?: boolean };
type MarkSerializerSpec = MarkdownSerializer["marks"][string];

const link: MarkSerializerSpec = {
  open(state, mark, parent, index) {
    if (!needsAngleBrackets(mark.attrs.href)) return defaultLink.open(state, mark, parent, index);
    (state as LinkState).inAutolink = false;
    return "[";
  },
  close(state, mark, parent, index) {
    const { href, title } = mark.attrs;
    if (!needsAngleBrackets(href)) return defaultLink.close(state, mark, parent, index);
    (state as LinkState).inAutolink = undefined;
    const quotedTitle = title ? ` "${title.replace(/"/g, '\\"')}"` : "";
    return `](<${href.replace(/[<>\\]/g, "\\$&").replace(/\n/g, " ")}>${quotedTitle})`;
  },
  mixable: true,
};

const serializer = new MarkdownSerializer(
  {
    blockquote: defaults.nodes.blockquote,
    heading: defaults.nodes.heading,
    paragraph: defaults.nodes.paragraph,
    list_item: defaults.nodes.list_item,
    ordered_list: defaults.nodes.ordered_list,
    hard_break: defaults.nodes.hard_break,
    text: defaults.nodes.text,
    horizontal_rule(state, node) {
      state.write("---");
      state.closeBlock(node);
    },
    bullet_list(state, node) {
      state.renderList(node, "  ", () => "- ");
    },
    wikilink(state, node) {
      const { target, label } = node.attrs;
      state.write(`[[${target}${label ? `|${label}` : ""}]]`);
    },
  },
  {
    link,
    em: defaults.marks.em,
    strong: defaults.marks.strong,
    underline: { open: "<u>", close: "</u>", mixable: true, expelEnclosingWhitespace: true },
  },
);

// The stock escaping backslashes every `[`, `]`, `~` and backtick, which would turn "[sic]" into
// "\[sic\]" the first time a file is saved. Prose files should stay readable, so escape only
// what would actually change meaning under the rules enabled above. prosemirror-markdown has no
// option for this and no public way to supply a state class, hence the prototype override.
MarkdownSerializerState.prototype.esc = function (str: string, startOfLine = false): string {
  str = str.replace(/[\\*_[\]&<]/g, (m, i: number) => {
    switch (m) {
      case "<":
        // Typed `<u>` would come back as underlining.
        return /^<\/?u>/.test(str.slice(i)) ? "\\<" : m;
      case "&":
        // `&amp;` typed as text would come back as `&`.
        return /^&(?:#\d+|#[xX][\da-fA-F]+|[A-Za-z][A-Za-z\d]*);/.test(str.slice(i)) ? "\\&" : m;
      case "_":
        return isWordChar(str[i - 1]) && isWordChar(str[i + 1]) ? m : "\\_";
      case "[":
        // `[[` would start a wikilink.
        return str[i + 1] === "[" ? "\\[" : m;
      case "]":
        // `](` and `][` would close a link.
        return str[i + 1] === "(" || str[i + 1] === "[" ? "\\]" : m;
      default:
        return "\\" + m;
    }
  });
  if (startOfLine) {
    str = str
      .replace(/^(\+[ ]|[-*>])/, "\\$&")
      .replace(/^(\s*)(#{1,6})(\s|$)/, "$1\\$2$3")
      .replace(/^(\s*\d+)([.)])(\s|$)/, "$1\\$2$3");
  }
  return str;
};

function isWordChar(ch: string | undefined): boolean {
  return ch !== undefined && /\w/.test(ch);
}

export function parseMarkdown(markdown: string): Node {
  return parser.parse(markdown);
}

/** Canonical Markdown for a document, ending with a single newline (or empty). */
export function serializeMarkdown(doc: Node): string {
  // Trailing spaces never carry meaning here (hard breaks are written as `\`), and the parser
  // drops them, so writing them would make the file change again on the next save.
  const out = serializer.serialize(doc).replace(/[ \t]+$/gm, "");
  return out === "" ? "" : out + "\n";
}

const WORD = /[\p{L}\p{N}]+(?:['’-][\p{L}\p{N}]+)*/gu;

export function countWords(doc: Node): number {
  return doc.textBetween(0, doc.content.size, "\n").match(WORD)?.length ?? 0;
}
