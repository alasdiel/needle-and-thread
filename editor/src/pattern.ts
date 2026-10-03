// Pattern markings drawn over the text: the cut line that previews where Split will cut, and the
// seam that marks where Merge joined a scene on. They're decorations only; the document never
// changes, and the app (crates/desktop/styles/app.css) styles them.
//
// The scissors and needle are line icons; the scissors come from Lucide (ISC), whose licence is
// in crates/desktop/assets/licenses/lucide-LICENSE.txt.

import type { Node, ResolvedPos } from "prosemirror-model";
import { type EditorState, Plugin, PluginKey } from "prosemirror-state";
import { Decoration, DecorationSet } from "prosemirror-view";

export interface CutCallbacks {
  onCutConfirm: () => void;
  onCutCancel: () => void;
}

/** How long the seam stays, in milliseconds; the CSS fades it out over the end of this. */
export const SEAM_MS = 6000;

const ICON = `width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"`;
const SCISSORS = `<svg ${ICON}><circle cx="6" cy="6" r="3"/><path d="M8.12 8.12 12 12"/><path d="M20 4 8.12 15.88"/><circle cx="6" cy="18" r="3"/><path d="M14.8 14.8 20 20"/></svg>`;
const NEEDLE = `<svg ${ICON}><path d="M4.5 19.5 16 8"/><ellipse cx="18" cy="6" rx="1.3" ry="3" transform="rotate(45 18 6)"/><path d="M18.8 5.2c3 1.5 1.5 6-2.5 7.5S8.5 15 9.5 19"/></svg>`;

/** Top-level blocks in `doc`, or 0 when it's empty (a lone empty paragraph). */
export function blockCount(doc: Node): number {
  return doc.childCount === 1 && doc.firstChild!.content.size === 0 ? 0 : doc.childCount;
}

/** The position just before top-level block `index`, or the end if there are fewer blocks. */
export function seamPosition(doc: Node, index: number): number {
  let pos = 0;
  for (let i = 0; i < Math.min(index, doc.childCount); i++) pos += doc.child(i).nodeSize;
  return pos;
}

// --- The cut line ---------------------------------------------------------------------------

/** Where to draw the cut line for a cursor at `$pos`. At either end of a top-level paragraph
 * it goes between blocks, since drawn inside the paragraph it would leave an empty line behind;
 * elsewhere it splits the line at the cursor, as the cut will. */
export function cutLinePosition($pos: ResolvedPos): number {
  if ($pos.depth === 1) {
    if ($pos.parentOffset === $pos.parent.content.size) return $pos.after();
    if ($pos.parentOffset === 0) return $pos.before();
  }
  return $pos.pos;
}

export const cutLineKey = new PluginKey<boolean>("cutLine");

/** While on, a cut line follows the cursor; Enter confirms and Escape cancels. */
export function cutLinePlugin(callbacks: () => CutCallbacks): Plugin<boolean> {
  return new Plugin<boolean>({
    key: cutLineKey,
    state: {
      init: () => false,
      apply: (tr, on) => {
        const meta = tr.getMeta(cutLineKey);
        return typeof meta === "boolean" ? meta : on;
      },
    },
    props: {
      decorations(state: EditorState) {
        if (!cutLineKey.getState(state)) return null;
        const widget = Decoration.widget(cutLinePosition(state.selection.$from), () => cutLineDOM(callbacks()), {
          side: 1,
          key: "cut-line",
          ignoreSelection: true,
          stopEvent: () => true,
        });
        return DecorationSet.create(state.doc, [widget]);
      },
      handleKeyDown(view, event) {
        if (!cutLineKey.getState(view.state)) return false;
        if (event.key === "Enter") {
          callbacks().onCutConfirm();
          return true;
        }
        if (event.key === "Escape") {
          callbacks().onCutCancel();
          return true;
        }
        return false;
      },
    },
  });
}

function cutLineDOM(callbacks: CutCallbacks): HTMLElement {
  const line = document.createElement("span");
  line.className = "cut-line";
  line.contentEditable = "false";
  line.innerHTML = `${SCISSORS}<span class="cut-dashes"></span>`;
  const actions = document.createElement("span");
  actions.className = "cut-actions";
  actions.append(actionButton("Cut here", "Enter", "primary", callbacks.onCutConfirm));
  actions.append(actionButton("Cancel", "Esc", "", callbacks.onCutCancel));
  line.append(actions);
  return line;
}

function actionButton(label: string, key: string, className: string, onClick: () => void): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = className;
  const kbd = document.createElement("kbd");
  kbd.textContent = key;
  button.append(label, kbd);
  // Keep the editor's cursor, which says where to cut.
  button.addEventListener("mousedown", (event) => event.preventDefault());
  button.addEventListener("click", onClick);
  return button;
}

// --- The seam --------------------------------------------------------------------------------

export const seamKey = new PluginKey<DecorationSet>("seam");

/** A position to draw the seam at, or "clear" to take it away. */
export type SeamMeta = number | "clear";

export function seamPlugin(): Plugin<DecorationSet> {
  return new Plugin<DecorationSet>({
    key: seamKey,
    state: {
      init: () => DecorationSet.empty,
      apply: (tr, seams) => {
        const meta = tr.getMeta(seamKey) as SeamMeta | undefined;
        if (meta === "clear") return DecorationSet.empty;
        if (typeof meta === "number") {
          const widget = Decoration.widget(meta, seamDOM, { side: -1, key: "seam", ignoreSelection: true });
          return DecorationSet.create(tr.doc, [widget]);
        }
        return seams.map(tr.mapping, tr.doc);
      },
    },
    props: {
      decorations: (state: EditorState) => seamKey.getState(state),
    },
  });
}

function seamDOM(): HTMLElement {
  const seam = document.createElement("div");
  seam.className = "seam";
  seam.contentEditable = "false";
  seam.innerHTML = `<span class="seam-stitches"></span><span class="seam-label">Seam · the next scene starts here</span>${NEEDLE}`;
  return seam;
}
