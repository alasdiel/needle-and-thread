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
- Reading and writing on your phone: the outline, scenes, notes, links and search, straight from the vault's GitHub repository (see below)
- Sharing: a link to a scene, chapter, part or the whole book that readers open in a browser and comment on, always showing the latest snapshot. The text and comments are encrypted with a key that's only in the link. Comments show on their words in the editor, and follow them as you rewrite

## Install it

You'll need Rust (stable), a C compiler, [trunk](https://trunkrs.dev), the [Tauri CLI](https://tauri.app) (`cargo install --locked tauri-cli --version '^2'`), pnpm, and on Linux, WebKitGTK.

```sh
pnpm --dir editor install
scripts/install.sh
```

That builds it and puts it under `~/.local`, so it starts from your desktop's menu, or as `needle-and-thread`, with no dev server running. Pull and run it again to update; `scripts/install.sh --uninstall` takes it off and leaves your vault alone.

Pick a vault on first launch, or try the sample.

## On your phone

The phone app reads and writes your vault in the GitHub repository it backs up to. Open it at `https://<you>.github.io/needle-and-thread/` once Pages is on for this repository, and add it to your home screen. It asks for the vault's repository (`you/your-vault`) and a [fine-grained token](https://github.com/settings/personal-access-tokens/new) with access to only that repository and **Contents: read and write** (a read-only token reads fine, but can't save). Both stay on the phone.

A scene or note opens to read; **Write** turns it into the desktop's editor, with a bar of formats above the keyboard. What you write is kept on the phone as you pause and goes to GitHub as one commit after a little longer, or when you tap Done. If the desktop changed the same file meanwhile, the two are merged a paragraph at a time; where both changed the same paragraph, the phone's version stays and the desktop's is kept for you to settle there, as with sync.

## Sharing

Links are made in the Shared panel (the link button in the top bar) and open the reader page, published with the phone app at `https://<you>.github.io/needle-and-thread/read/`. Readers' comments are kept, encrypted, by a small Cloudflare Worker (`crates/worker`); `.github/workflows/worker.yml` deploys it with the repository secrets `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID`. The links' keys are kept in the vault (`.needle/shares.toml`), so keep the vault's repository private.

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

The phone app has its own suite, against a stand-in for GitHub: `trunk serve` in `crates/phone` (port 1421), then `pnpm --dir e2e test:phone`.

The reader page's suite runs against a real Worker on your machine: `pnpm --dir crates/worker install && pnpm --dir crates/worker dev` (port 8787), `trunk serve` in `crates/reader` (port 1422), then `pnpm --dir e2e test:reader`.

## Layout

| Path | What |
|---|---|
| `crates/core` | Shared model: scene and note files, headers, outline, links and names, diff, spelling |
| `crates/vault` | Reading and writing vaults: projects, scenes, notes, worlds, on disk or held in memory |
| `crates/vcs` | Snapshots, history and backup pushes (git) |
| `crates/index` | The search index (SQLite), a cache kept outside the vault |
| `crates/desktop` | The Tauri app: Leptos frontend, backend in `src-tauri/` |
| `crates/phone` | The phone app (Leptos, published to GitHub Pages): reads and writes the vault on GitHub |
| `crates/reader` | The reader page share links open (Leptos, published with the phone app under `read/`) |
| `crates/worker` | The sharing Worker (Cloudflare, workers-rs): sealed text and comments |
| `editor/` | The ProseMirror editor (TypeScript) |
| `e2e/` | Browser tests of the interface (Playwright), against `trunk serve` |
| `docs/` | Design document and spike findings |
