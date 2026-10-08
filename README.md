# Needle and Thread

A structure-first writing app for long fiction and nonfiction. Your work lives in a vault: a folder of plain Markdown files, with a history of snapshots kept alongside. No AI.

**Status:** early. Writing, restructuring, notes, links, search, history, backup and the network board work; the timeline (DESIGN §6) is next. See [docs/DESIGN.md](docs/DESIGN.md) for the plan.
It installs from source for now; packaged downloads come later.

## What works

- A Medium-style editor that saves plain Markdown as you pause (and on closing the window), with spellcheck and optional smart typography (each switch saved with the vault)
- An outline of parts, chapters and scenes: drag to restructure, split and merge scenes
- A cut bin instead of deleting: cut a passage (right-click, or Ctrl+Shift+X), a scene or a note, and restore it later to where it was
- Notes for characters, places, threads, sources, plot points and anything else, each type starting from a template you can edit
- `[[Links]]` to notes, suggested as you type. Renaming a note keeps every link to it, and each note lists what links to it and where it's mentioned without a link
- Each scene's summary, POV, cast, places and threads on an envelope beside it, with hints for names the text uses but doesn't list
- Worlds: characters, places and history shared by a series of projects
- Search across scenes and notes (Ctrl+K, or the Search tab), filtered by status, POV or thread
- Automatic snapshots (git under the hood), with history by day and session, compare, restore, and putting back a single removed passage, for every scene and note
- Backup to GitHub over SSH after each snapshot, with its status beside "Saved"
- A network board: a cork board of characters, places, plot points and threads, with relationships as red string you tie between them, zones of kraft paper, and your own handwritten notes and marker
- A sewing-pattern look, light or dark

## Install it

You'll need Rust (stable), a C compiler, [trunk](https://trunkrs.dev), the [Tauri CLI](https://tauri.app) (`cargo install --locked tauri-cli --version '^2'`), pnpm, and on Linux, WebKitGTK and a Hunspell `en_US` dictionary.

```sh
pnpm --dir editor install
scripts/install.sh
```

That builds it and puts it under `~/.local`, so it starts from your desktop's menu, or as `needle-and-thread`, with no dev server running. Pull and run it again to update; `scripts/install.sh --uninstall` takes it off and leaves your vault alone.

Pick a vault on first launch, or try the sample.

## Work on it

```sh
cd crates/desktop && cargo tauri dev
```

Reloads the interface as you edit it. It wants port 1420, so stop an installed copy first if it's running.

## Test

```sh
cargo test --workspace --exclude needle-desktop-ui
pnpm --dir editor test
```

The network board is also driven in a browser (`e2e/`), with a stand-in for the backend. Start `trunk serve` in `crates/desktop`, then:

```sh
pnpm --dir e2e install && pnpm --dir e2e exec playwright install chromium-headless-shell
pnpm --dir e2e test
```

## Layout

| Path | What |
|---|---|
| `crates/core` | Shared model: scene and note files, headers, outline, links and names, diff, spelling |
| `crates/vault` | Reading and writing vaults on disk: projects, scenes, notes, worlds |
| `crates/vcs` | Snapshots, history and backup pushes (git) |
| `crates/index` | The search index (SQLite), a cache kept outside the vault |
| `crates/desktop` | The Tauri app: Leptos frontend, backend in `src-tauri/` |
| `editor/` | The ProseMirror editor (TypeScript) |
| `e2e/` | Browser tests of the interface (Playwright), against `trunk serve` |
| `docs/` | Design document and spike findings |
