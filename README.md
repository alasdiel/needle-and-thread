# Needle and Thread

A structure-first writing app for long fiction and nonfiction. Your work lives in a vault: a folder of plain Markdown files, with a history of snapshots kept alongside. No AI.

**Status:** early. Writing, restructuring, notes, links and search work; a command palette and exports are next. See [docs/DESIGN.md](docs/DESIGN.md) for the plan.
Built releases will also come later.

## What works

- A Medium-style editor that saves plain Markdown as you pause (and on closing the window), with spellcheck and optional smart typography
- An outline of parts, chapters and scenes: drag to restructure, split and merge scenes
- A cut bin instead of deleting: cut a passage (right-click, or Ctrl+Shift+X), a scene or a note, and restore it later to where it was
- Notes for characters, places, threads, sources, plot points and anything else, each type starting from a template you can edit
- `[[Links]]` to notes, suggested as you type. Renaming a note keeps every link to it, and each note lists what links to it and where it's mentioned without a link
- Each scene's POV, cast, places and threads, with hints for names the text uses but doesn't list
- Worlds: characters, places and history shared by a series of projects
- Search across scenes and notes (Ctrl+K, or the Search tab), filtered by status, POV or thread
- Automatic snapshots (git under the hood), with history, compare and restore for every scene and note
- A sewing-pattern look, light or dark

## Run it

You'll need Rust (stable), a C compiler, [trunk](https://trunkrs.dev), the [Tauri CLI](https://tauri.app) (`cargo install tauri-cli`), pnpm, and on Linux, WebKitGTK and a Hunspell `en_US` dictionary.

```sh
pnpm --dir editor install
cd crates/desktop && cargo tauri dev
```

Pick a vault on first launch, or try the sample.

## Test

```sh
cargo test --workspace --exclude needle-desktop-ui
pnpm --dir editor test
```

## Layout

| Path | What |
|---|---|
| `crates/core` | Shared model: scene and note files, headers, outline, links and names, diff, spelling |
| `crates/vault` | Reading and writing vaults on disk: projects, scenes, notes, worlds |
| `crates/vcs` | Snapshots and history (git) |
| `crates/index` | The search index (SQLite), a cache kept outside the vault |
| `crates/desktop` | The Tauri app: Leptos frontend, backend in `src-tauri/` |
| `editor/` | The ProseMirror editor (TypeScript) |
| `docs/` | Design document and spike findings |
