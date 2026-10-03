# Needle and Thread

A structure-first writing app for long fiction and nonfiction. Your work lives in a vault: a folder of plain Markdown files, with a history of snapshots kept alongside. No AI.

**Status:** early. The editor, snapshots and the outline work; notes, search and exports are next. See [docs/DESIGN.md](docs/DESIGN.md) for the plan.

## What works

- A Medium-style editor that saves plain Markdown, with spellcheck and optional smart typography
- An outline of parts, chapters and scenes: drag to restructure, split and merge scenes, a cut bin instead of deleting
- Automatic snapshots (git under the hood), with history, compare and restore

## Run it

You'll need Rust (stable), [trunk](https://trunkrs.dev), the [Tauri CLI](https://tauri.app) (`cargo install tauri-cli`), pnpm, and on Linux, WebKitGTK and a Hunspell `en_US` dictionary.

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
| `crates/core` | Shared model: scene files, headers, outline, diff, spelling |
| `crates/vault` | Reading and writing vaults on disk |
| `crates/vcs` | Snapshots and history (git) |
| `crates/desktop` | The Tauri app: Leptos frontend, backend in `src-tauri/` |
| `editor/` | The ProseMirror editor (TypeScript) |
| `docs/` | Design document and spike findings |
