# Spike 1 — Editor island

*2026-10-03 · Tauri 2.12, Leptos 0.8.21, ProseMirror (prosemirror-markdown 1.13), Rust 1.99, WebKitGTK 2.52*

## Answers

**Can ProseMirror run inside Leptos/Tauri behind a thin `wasm-bindgen` boundary?** Yes.
- **Loading:** the editor is bundled as an IIFE (`window.NeedleEditor`) and loaded with a classic `<script>` before trunk's wasm loader.
- **Bindings:** they're in `crates/desktop/src/editor.rs`. Closures passed to JS live in `StoredValue::new_local`.
- **Changes:** reported debounced (1 s) and flushed on blur and before switching scenes.

**Does Markdown round-trip cleanly?** Yes. Canonical files, both sample scenes and real test edits save byte-identical except for the edited text. The stock serializer needed fixes:
- `[sic]` was escaped to `\[sic\]`. Escaping is now limited to what would change meaning.
- markdown-it percent-encoded URLs (`café` → `caf%C3%A9`). URLs are now kept as written.
- Typed `&amp;` and `<u>` came back as `&` and underline. These are now escaped.
- Trailing spaces made files change on the next save. They're no longer written.

**Does spellcheck work in WebKitGTK?** Only for newly typed text, and there's no way to check a whole document. We replaced it:
- spellbook (Rust, Hunspell format) runs in the wasm frontend, and the editor draws the underlines (`editor/src/spellcheck.ts`).
- Loading the system en_US dictionary takes about 18 ms.

## Other findings

- **Rendering:** fine on Hyprland/Wayland with a hybrid NVIDIA GPU. No `WEBKIT_DISABLE_DMABUF_RENDERER` needed.
- **Performance:** no lag typing in a 10k-word scene.
- **IME:** untested, since no input method is installed.
- **Toolchain:**
  - `cargo install --locked trunk` fails under GCC 16 (libdeflate-sys 1.23). Build it from source with `libdeflate-sys` bumped to 1.26.
  - tauri-cli 2.12 needs rustc ≥ 1.90.
- **Disk:** dependency debug info made `target/` 5+ GB. The dev profile now skips it (about 1.9 GB).

## Changes from testing

- Shift+Enter in a list starts a sublist.
- Scene breaks are typed and stored as `---`.
- Ctrl+U underlines, stored as `<u>…</u>`.
- Typography is four separate settings.

## Known limits, for phase 1

- **Front matter on every scene:** a scene file with no front matter that starts with a `---` scene break would be read as front matter, so every scene must have some (at least `id`).
- **Adjacent lists:** two lists of the same kind directly after each other merge on reload.
- **Leading spaces:** leading spaces in a paragraph are dropped.
- **Saving on close:** closing the window within 1 s of typing can lose that second; flush on close.
- **Release build:** not measured yet. Low disk space.
