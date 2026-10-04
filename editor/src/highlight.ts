// Highlights the words of a search in the open scene or note, the way search matches them:
// each word typed matches the start of a word, ignoring case.

import type { Node } from "prosemirror-model";
import { Plugin, PluginKey } from "prosemirror-state";
import { Decoration, DecorationSet } from "prosemirror-view";

export type HighlightMeta = { terms: string[] };

interface HighlightState {
  pattern: RegExp | null;
  decorations: DecorationSet;
}

export const highlightKey = new PluginKey<HighlightState>("search-highlight");

/** A pattern matching any word that starts with one of `terms`, or null for none. */
export function termPattern(terms: string[]): RegExp | null {
  const words = terms
    .flatMap((t) => t.split(/[^\p{L}\p{N}]+/u))
    .filter((w) => w.length > 0)
    .map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  if (!words.length) return null;
  return new RegExp(`(?<![\\p{L}\\p{N}])(?:${words.join("|")})[\\p{L}\\p{N}]*`, "giu");
}

function decorate(doc: Node, pattern: RegExp | null): DecorationSet {
  if (!pattern) return DecorationSet.empty;
  const found: Decoration[] = [];
  doc.descendants((node, pos) => {
    if (!node.isText) return true;
    for (const match of node.text!.matchAll(pattern)) {
      const from = pos + match.index;
      found.push(Decoration.inline(from, from + match[0].length, { class: "search-match" }));
    }
    return false;
  });
  return DecorationSet.create(doc, found);
}

export function highlightPlugin(): Plugin<HighlightState> {
  return new Plugin<HighlightState>({
    key: highlightKey,
    state: {
      init: () => ({ pattern: null, decorations: DecorationSet.empty }),
      apply(tr, prev, _old, state) {
        const meta = tr.getMeta(highlightKey) as HighlightMeta | undefined;
        if (meta) {
          const pattern = termPattern(meta.terms);
          return { pattern, decorations: decorate(state.doc, pattern) };
        }
        if (!tr.docChanged) return prev;
        // Recomputed rather than mapped, so newly typed matches light up too.
        return { pattern: prev.pattern, decorations: decorate(state.doc, prev.pattern) };
      },
    },
    props: {
      decorations: (state) => highlightKey.getState(state)?.decorations,
    },
  });
}
