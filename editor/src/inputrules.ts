import {
  InputRule,
  closeDoubleQuote,
  closeSingleQuote,
  ellipsis,
  emDash,
  inputRules,
  openDoubleQuote,
  openSingleQuote,
  textblockTypeInputRule,
  wrappingInputRule,
} from "prosemirror-inputrules";
import { Fragment } from "prosemirror-model";
import { type Plugin, TextSelection } from "prosemirror-state";
import { schema } from "./schema.ts";

const { nodes } = schema;

// `---` alone on a line becomes a scene break. With the em dash rule on, the first two hyphens
// have already become an em dash by the time the third is typed, hence `—-`.
const sceneBreak = new InputRule(/^(?:---|—-)$/, (state, _match, start, end) => {
  const $start = state.doc.resolve(start);
  const paragraph = $start.parent;
  const wholeParagraph = $start.parentOffset === 0 && state.doc.resolve(end).parentOffset === paragraph.content.size;
  if (paragraph.type !== nodes.paragraph || !wholeParagraph) return null;
  const from = $start.before();
  const tr = state.tr.replaceWith(
    from,
    $start.after(),
    Fragment.from([nodes.horizontal_rule.create(), nodes.paragraph.create()]),
  );
  return tr.setSelection(TextSelection.create(tr.doc, from + 2));
});

const wikilink = new InputRule(/\[\[([^[\]|\n]+)(?:\|([^[\]\n]+))?\]\]$/, (state, match, start, end) => {
  const target = match[1].trim();
  if (!target) return null;
  return state.tr.replaceWith(start, end, nodes.wikilink.create({ target, label: match[2]?.trim() || null }));
});

/** Each automatic typography change, switchable on its own. They only affect typing; existing
 * text is never rewritten. */
export interface Typography {
  /** "…" → “…” */
  doubleQuotes: boolean;
  /** '…' → ‘…’, and it's → it’s */
  singleQuotes: boolean;
  /** -- → — */
  emDash: boolean;
  /** ... → … */
  ellipsis: boolean;
}

export const defaultTypography: Typography = {
  doubleQuotes: true,
  singleQuotes: true,
  emDash: true,
  ellipsis: true,
};

export function buildInputRules(typography: Typography): Plugin {
  const rules = [
    textblockTypeInputRule(/^(#{1,3})\s$/, nodes.heading, (match) => ({ level: match[1].length })),
    wrappingInputRule(/^\s*>\s$/, nodes.blockquote),
    wrappingInputRule(/^\s*[-+]\s$/, nodes.bullet_list),
    wrappingInputRule(
      /^(\d+)\.\s$/,
      nodes.ordered_list,
      (match) => ({ order: Number(match[1]) }),
      (match, node) => node.childCount + node.attrs.order === Number(match[1]),
    ),
    sceneBreak,
    wikilink,
  ];
  if (typography.doubleQuotes) rules.push(openDoubleQuote, closeDoubleQuote);
  if (typography.singleQuotes) rules.push(openSingleQuote, closeSingleQuote);
  if (typography.emDash) rules.push(emDash);
  if (typography.ellipsis) rules.push(ellipsis);
  return inputRules({ rules });
}
