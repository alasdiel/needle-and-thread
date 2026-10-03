// Right-click menu for misspelled words: suggestions, plus "Add to dictionary".

import type { EditorView } from "prosemirror-view";
import { type SpellMeta, spellcheckKey } from "./spellcheck.ts";

const MAX_SUGGESTIONS = 6;

interface MenuItem {
  label: string;
  run?: () => void;
}

/** Opens the menu if `event` is on a misspelled word. Returns false to leave the native menu. */
export function openSpellMenu(view: EditorView, event: MouseEvent): boolean {
  const spell = spellcheckKey.getState(view.state);
  const at = view.posAtCoords({ left: event.clientX, top: event.clientY });
  if (!spell?.session || !at) return false;
  const [hit] = spell.decorations.find(at.pos, at.pos);
  if (!hit) return false;

  event.preventDefault();
  const { session } = spell;
  const { from, to } = hit;
  const word: string = hit.spec.word;
  // Only act if the word is still where the menu was opened.
  const stillThere = () => view.state.doc.textBetween(from, to) === word;

  const suggestions = session.checker.suggest(word).slice(0, MAX_SUGGESTIONS);
  const items: (MenuItem | "separator")[] = suggestions.length
    ? suggestions.map((s) => ({
        label: s,
        run: () => stillThere() && view.dispatch(view.state.tr.insertText(s, from, to)),
      }))
    : [{ label: "No suggestions" }];
  items.push("separator", {
    label: `Add “${word}” to dictionary`,
    run: () => {
      session.checker.add(word);
      const meta: SpellMeta = { recheck: true };
      view.dispatch(view.state.tr.setMeta(spellcheckKey, meta));
    },
  });

  showMenu(view, event.clientX, event.clientY, items);
  return true;
}

let closeOpenMenu: (() => void) | null = null;

function showMenu(view: EditorView, x: number, y: number, items: (MenuItem | "separator")[]): void {
  closeOpenMenu?.();

  const menu = document.createElement("div");
  menu.className = "needle-menu";
  menu.setAttribute("role", "menu");
  for (const item of items) {
    if (item === "separator") {
      menu.append(document.createElement("hr"));
      continue;
    }
    const button = document.createElement("button");
    button.type = "button";
    button.setAttribute("role", "menuitem");
    button.textContent = item.label;
    button.disabled = !item.run;
    button.addEventListener("click", () => {
      close();
      item.run?.();
    });
    menu.append(button);
  }
  document.body.append(menu);

  // Keep it on screen.
  const { width, height } = menu.getBoundingClientRect();
  menu.style.left = `${Math.max(4, Math.min(x, window.innerWidth - width - 4))}px`;
  menu.style.top = `${Math.max(4, Math.min(y, window.innerHeight - height - 4))}px`;

  const buttons = [...menu.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
  buttons[0]?.focus();

  const onPointerDown = (e: Event) => {
    if (!menu.contains(e.target as globalThis.Node)) close();
  };
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const i = buttons.indexOf(document.activeElement as HTMLButtonElement);
      const next = e.key === "ArrowDown" ? i + 1 : i - 1;
      buttons[(next + buttons.length) % buttons.length]?.focus();
    }
  };
  function close() {
    menu.remove();
    document.removeEventListener("pointerdown", onPointerDown, true);
    document.removeEventListener("keydown", onKeyDown, true);
    window.removeEventListener("blur", close);
    closeOpenMenu = null;
    view.focus();
  }
  document.addEventListener("pointerdown", onPointerDown, true);
  document.addEventListener("keydown", onKeyDown, true);
  window.addEventListener("blur", close);
  closeOpenMenu = close;
}
