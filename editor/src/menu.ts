// Right-click menus: for misspelled words, suggestions and "Add to dictionary"; for selected
// text in a scene, "Cut to bin" above the usual Cut, Copy and Paste.

import type { EditorView } from "prosemirror-view";
import { type SpellMeta, spellcheckKey } from "./spellcheck.ts";

const MAX_SUGGESTIONS = 6;

// The app's basket icon (crates/desktop/src/icons.rs).
const BASKET = `<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 10h16l-1.5 9.2a1 1 0 0 1-1 .8h-11a1 1 0 0 1-1-.8L4 10Z"/><path d="M8.5 10 11 4"/><path d="M15.5 10 13 4"/><path d="M9 14v3"/><path d="M12 14v3"/><path d="M15 14v3"/></svg>`;

/** A menu entry; one without `run` is shown greyed out, as a heading or a note. */
export interface MenuItem {
  label: string;
  run?: () => void;
  /** An SVG icon before the label. */
  icon?: string;
  /** Its shortcut, shown at the right. */
  shortcut?: string;
}

/** Opens the selection's menu if `event` is on selected text. Returns false otherwise. */
export function openSelectionMenu(view: EditorView, event: MouseEvent, cutToBin: () => void): boolean {
  const { selection } = view.state;
  const at = view.posAtCoords({ left: event.clientX, top: event.clientY });
  if (selection.empty || !at || at.pos < selection.from || at.pos > selection.to) return false;
  event.preventDefault();
  showMenu(view, event.clientX, event.clientY, [
    { label: "Cut to bin", icon: BASKET, shortcut: "Ctrl+Shift+X", run: cutToBin },
    "separator",
    { label: "Cut", shortcut: "Ctrl+X", run: () => document.execCommand("cut") },
    { label: "Copy", shortcut: "Ctrl+C", run: () => document.execCommand("copy") },
    { label: "Paste", shortcut: "Ctrl+V", run: () => void paste(view) },
  ]);
  return true;
}

async function paste(view: EditorView): Promise<void> {
  try {
    view.pasteText(await navigator.clipboard.readText());
  } catch {
    document.execCommand("paste");
  }
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

export function showMenu(view: EditorView, x: number, y: number, items: (MenuItem | "separator")[]): void {
  closeOpenMenu?.();

  const menu = document.createElement("div");
  menu.className = "needle-menu";
  menu.setAttribute("role", "menu");
  // When one entry has an icon, the others leave room for one, so the labels line up.
  const icons = items.some((item) => item !== "separator" && item.icon);
  for (const item of items) {
    if (item === "separator") {
      menu.append(document.createElement("hr"));
      continue;
    }
    const button = document.createElement("button");
    button.type = "button";
    button.setAttribute("role", "menuitem");
    if (icons || item.shortcut) {
      button.className = "with-parts";
      if (icons) {
        const icon = document.createElement("span");
        icon.className = "menu-icon";
        icon.innerHTML = item.icon ?? "";
        button.append(icon);
      }
      button.append(item.label);
      if (item.shortcut) {
        const kbd = document.createElement("kbd");
        kbd.textContent = item.shortcut;
        button.append(kbd);
      }
    } else {
      button.textContent = item.label;
    }
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
