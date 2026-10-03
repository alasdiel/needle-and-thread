import { Schema } from "prosemirror-model";

// Prose only: no code, images or raw HTML. Anything the schema can't hold would be lost on save.
export const schema = new Schema({
  nodes: {
    doc: { content: "block+" },

    paragraph: {
      content: "inline*",
      group: "block",
      parseDOM: [{ tag: "p" }],
      toDOM: () => ["p", 0],
    },

    // Levels 4–6 aren't offered in the UI, but files may contain them and must round-trip.
    heading: {
      attrs: { level: { default: 1 } },
      content: "inline*",
      group: "block",
      defining: true,
      parseDOM: [1, 2, 3, 4, 5, 6].map((level) => ({ tag: `h${level}`, attrs: { level } })),
      toDOM: (node) => [`h${node.attrs.level}`, 0],
    },

    blockquote: {
      content: "block+",
      group: "block",
      defining: true,
      parseDOM: [{ tag: "blockquote" }],
      toDOM: () => ["blockquote", 0],
    },

    // A scene break. Stored as `---`.
    horizontal_rule: {
      group: "block",
      parseDOM: [{ tag: "hr" }],
      toDOM: () => ["hr", { class: "scene-break" }],
    },

    bullet_list: {
      attrs: { tight: { default: false } },
      content: "list_item+",
      group: "block",
      parseDOM: [{ tag: "ul", getAttrs: (dom) => ({ tight: dom.hasAttribute("data-tight") }) }],
      toDOM: (node) => ["ul", node.attrs.tight ? { "data-tight": "true" } : {}, 0],
    },

    ordered_list: {
      attrs: { order: { default: 1 }, tight: { default: false } },
      content: "list_item+",
      group: "block",
      parseDOM: [
        {
          tag: "ol",
          getAttrs: (dom) => ({
            order: dom.hasAttribute("start") ? Number(dom.getAttribute("start")) : 1,
            tight: dom.hasAttribute("data-tight"),
          }),
        },
      ],
      toDOM: (node) => [
        "ol",
        {
          ...(node.attrs.order === 1 ? {} : { start: node.attrs.order }),
          ...(node.attrs.tight ? { "data-tight": "true" } : {}),
        },
        0,
      ],
    },

    list_item: {
      content: "paragraph block*",
      defining: true,
      parseDOM: [{ tag: "li" }],
      toDOM: () => ["li", 0],
    },

    text: { group: "inline" },

    hard_break: {
      inline: true,
      group: "inline",
      selectable: false,
      leafText: () => "\n",
      parseDOM: [{ tag: "br" }],
      toDOM: () => ["br"],
    },

    // `[[Target]]` or `[[Target|Label]]`: a link to a note, shown as a chip.
    wikilink: {
      inline: true,
      group: "inline",
      atom: true,
      attrs: { target: {}, label: { default: null } },
      leafText: (node) => node.attrs.label ?? node.attrs.target,
      parseDOM: [
        {
          tag: "span.wikilink[data-target]",
          getAttrs: (dom) => ({
            target: dom.getAttribute("data-target"),
            label: dom.getAttribute("data-label"),
          }),
        },
      ],
      toDOM: (node) => [
        "span",
        {
          class: "wikilink",
          "data-target": node.attrs.target,
          ...(node.attrs.label ? { "data-label": node.attrs.label } : {}),
        },
        node.attrs.label ?? node.attrs.target,
      ],
    },
  },

  marks: {
    link: {
      attrs: { href: {}, title: { default: null } },
      inclusive: false,
      parseDOM: [
        {
          tag: "a[href]",
          getAttrs: (dom) => ({ href: dom.getAttribute("href"), title: dom.getAttribute("title") }),
        },
      ],
      toDOM: (mark) => ["a", { href: mark.attrs.href, title: mark.attrs.title }, 0],
    },

    em: {
      parseDOM: [{ tag: "i" }, { tag: "em" }, { style: "font-style=italic" }],
      toDOM: () => ["em", 0],
    },

    strong: {
      parseDOM: [{ tag: "strong" }, { tag: "b" }, { style: "font-weight=bold" }],
      toDOM: () => ["strong", 0],
    },

    // Markdown has no underline syntax; stored as `<u>…</u>`.
    underline: {
      parseDOM: [{ tag: "u" }, { style: "text-decoration=underline" }],
      toDOM: () => ["u", 0],
    },
  },
});
