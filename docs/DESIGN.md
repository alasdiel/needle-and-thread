# Needle and Thread — Design

*Draft 1 · 2026-10-03 · written from the discovery discussion*

Needle and Thread is a structure-first writing app for long fiction and long-form nonfiction. It targets two problems: turning scattered ideas into a structure that holds together, and keeping a big project with many scenes, characters, places, sources and threads under control. It has no AI features.

## 1. Principles

1. **Plain files are the source of truth.** Every scene and note is a Markdown file with a short metadata header, so you can read and edit it without the app. The search index, backlinks and timeline are caches, rebuildable from the files at any time.
2. **Restructuring is cheap.** You plan loosely and restructure often, so the outline *is* the manuscript. Moving, splitting and merging scenes each take one action, and they never break links, history or comments.
3. **Nothing is lost.** The app autosaves whenever you pause and takes snapshots automatically. Cut text and deleted scenes go to a bin.
4. **No AI.** Text is never sent anywhere for analysis or generation.
5. **Free to run.** It uses GitHub (a private repo plus Pages) and Cloudflare's free tier, with no servers to maintain.

## 2. Decisions so far

| Topic | Decision |
|---|---|
| Writing | Fiction and long-form nonfiction. Both are a tree of pieces with notes and references attached |
| Problems to solve | Structure & ideas; managing big projects |
| AI | None |
| Planning style | Hybrid: loose plan, then heavy restructuring as the draft grows |
| Views | Outline tree, timeline, linked notes (no index cards) |
| Reference material | Characters, places & world, research & sources, threads & arguments |
| Time | Real dates, relative times, invented calendars and order-only placement, mixed within one project |
| Shared material | Series sharing a world; self-contained projects too |
| Network | A map you build: automatic links plus named relationships you draw, changing over story time. One per project, can include notes from the world and other projects (§7) |
| Plot points | One note type with events (backstory included); can sit on the timeline |
| Devices | One Linux desktop now. Phone for capture, tagging and lookup. More computers, including Windows, later |
| Desktop app | Tauri 2 + Leptos, in Rust |
| Editor | ProseMirror as a small JavaScript island (Rust has no mature rich-text editor) |
| Editor feel | Like Medium: formatting shows as formatting, no visible Markdown symbols |
| Typography as you type | Four separate settings, each explained in the app and on by default: curly double quotes, curly single quotes/apostrophes, `--` → em dash, `...` → ellipsis. They never rewrite existing text |
| Spellcheck | Our own, not the webview's: spellbook (Rust, Hunspell-compatible) with the installed US English (`en_US`) dictionary. Underlines everything as soon as a scene opens, accepts names used in `[[links]]`, right-click for suggestions or "Add to dictionary" (`.needle/dictionary.txt`) |
| Phone app | PWA on GitHub Pages, sharing Rust UI code with the desktop |
| Storage & sync | One private GitHub "vault" repo for all writing; any project can be moved out to its own repo later |
| Snapshots | Automatic and debounced like Google Docs version history, plus named versions |
| Revision safety nets | Snapshots, cut bin, status labels |
| Sharing | View-and-comment links that always show the latest snapshot, one shared discussion per link, end-to-end encrypted |
| Comment storage | Cloudflare Worker, free tier |
| Opening your own share links | In the browser, with author tools |
| Export | PDF, EPUB, .docx (including standard manuscript format), copy for Medium |
| Import | None for now |
| Build order | Core → exports → timeline → sync + phone → sharing |

## 3. System overview

```
┌──────────────────────────── your computer ────────────────────────────┐
│ Desktop app (Tauri)                                                   │
│   UI: Leptos (WASM) + ProseMirror editor                              │
│   Rust: core model · SQLite index · git snapshots · exports           │
│   vault/  ← plain files in a git working copy                         │
└───────────┬────────────────────────────────────────────┬──────────────┘
            │ git push / pull                            │ encrypted share uploads,
            ▼                                            ▼ comment sync
   GitHub: private vault repo                 Cloudflare Worker (KV + D1)
            ▲                                            ▲
            │ GitHub REST API                            │ encrypted reads + comments
┌───────────┴────────────────────────────────────────────┴──────────────┐
│ GitHub Pages site (public code, never any writing)                    │
│   /       phone app: capture · tag · lookup                           │
│   /read   reader page for share links                                 │
└───────────────────────────────────────────────────────────────────────┘
```

### Repositories

| Repo | Visibility | Holds |
|---|---|---|
| `needle-and-thread` | Public | The app's code. GitHub Actions builds the phone app and reader page and publishes them to Pages. Free Pages requires a public repo, so **writing never goes here.** |
| *vault* (name TBD) | Private | All your writing. |

### Code layout (one Cargo workspace)

```
needle-and-thread/
├── Cargo.toml        workspace
├── crates/
│   ├── core/         model, Markdown + metadata, links, timeline & calendars,
│   │                 share encryption, comment anchoring. No I/O; builds native and wasm32
│   ├── index/        SQLite cache: full-text search, backlinks, mentions, timeline
│   ├── vcs/          git2: snapshots, history, diffs, sync, move-out
│   ├── export/       PDF (Typst), EPUB, DOCX, HTML for Medium
│   ├── ui/           Leptos components shared by desktop and phone
│   ├── desktop/      Leptos frontend for the desktop app (built to wasm by trunk)
│   │   └── src-tauri/  Tauri 2 backend: commands, file I/O, webview setup
│   ├── phone/        Leptos PWA for GitHub Pages, including the /read page
│   └── worker/       Cloudflare Worker (workers-rs)
├── editor/           ProseMirror island (TypeScript, bundled to one JS file)
├── fixtures/         Markdown round-trip fixtures and a sample vault, shared by JS and Rust tests
└── docs/             this file, plus spike findings in docs/spikes/
```

`crates/desktop` follows the layout of the official create-tauri-app Leptos template, so the Tauri and Leptos docs apply as written. Crates are added as their phase starts (`index`, `vcs`, `export`, `ui`, `phone` and `worker` don't exist yet).

`core` is used by the desktop app, the phone app and the reader page. Parsing, link resolution, timeline maths and encryption therefore behave the same everywhere. The Worker reuses its API types.

## 4. The vault

### Layout

```
vault/
├── .needle/
│   ├── vault.toml               settings: status labels, snapshot timing
│   ├── shares.toml              share registry: ids, scopes, keys
│   └── index.json               small name index for the phone app (generated)
├── inbox/
│   └── 2026-10-03T142205Z-k3f9.md   one file per phone idea
├── worlds/
│   └── glass-coast/
│       ├── world.toml           name, calendar
│       └── characters/ places/ threads/ sources/ events/ relationships/
└── projects/
    └── tidewater/
        ├── project.toml         title, kind (fiction | nonfiction), world, calendar
        ├── outline.toml         reading order
        ├── network.toml         where you've placed things on the network map
        ├── manuscript/          one file per scene or section
        │   ├── the-harbour.md
        │   └── night-market.md
        ├── notes/
        │   └── characters/ places/ threads/ sources/ events/ relationships/
        └── cut/                 cut bin
```

### The outline is one file, and scene files never move

Reading order lives in `outline.toml`. A scene file keeps the name it was created with, even after you retitle it or move it to another chapter. The exact format gets settled in phase 0.

```toml
[[part]]
title = "Part One: Landfall"

[[part.chapter]]
title = "Arrival"
summary = "Mara reaches Tidewater and hears about the ledger."
scenes = ["the-harbour", "night-market"]
```

This way, moving a scene changes one line of the outline instead of renaming a chain of files. Because a scene's path never changes, its history, links and anchored comments stay attached to it.

### A scene

```markdown
---
id: sc_7f3k9q              # stable id, survives any rename
title: The night market
status: draft              # idea | draft | revised | done
summary: Mara trades the compass and learns the ledger has left port.
pov: Mara Venn
cast: [Mara Venn, Old Teodor]
places: [Night Market]
threads: [The missing ledger]
when: { from: The harbour, offset: +6h }
---

The market opened at dusk, as it always had…
```

The body holds only prose, which is exactly what gets exported. Who appears in the scene, where it happens and which threads it advances all go in the header, so the prose never fills up with link syntax. The app also detects character and place names, including their aliases, in the text and suggests adding them to the header.

### A note

```markdown
---
id: ch_2m8x1a
type: character
title: Mara Venn
aliases: [Mara, the Captain]
---

Harbour pilot, thirty-four. Lost her ship in [[The Drowning]]…
```

Note types:
- **character**
- **place**
- **thread**: a plot thread, or a line of argument in nonfiction
- **source**: author, title, year, URL, pages, quotes
- **event**: anything that happens, backstory included; can sit on the timeline and the network
- **relationship**: a named line between two notes on the network (§7)
- **note**: anything else

Each type has its own template. Labels depend on the project's kind: fiction uses Scene, Thread and Plot point; nonfiction uses Section, Argument and Event.

### Links and shared worlds

- `[[Name]]` or `[[Name|shown text]]` links to any note, and typing `[[` brings up autocomplete.
- A name is looked up in the project first, then in the project's world. Self-contained projects have no world.
- A note in another project is linked with its project's folder name: `[[tidewater/Mara Venn]]`.
- Renaming a note updates every link that points to it.
- Each note shows three lists:
  - **backlinks**: what links to it
  - **appearances**: scenes that list it in their header
  - **unlinked mentions**: scenes that use its name or aliases
- A project can belong to one **world**. The world holds the characters, places, history and calendar that a series shares.
- **Promote to world** moves a note from a project into its world, for example when book two starts.
- For nonfiction, a world can serve as a shared research library.

### Cut bin

- Select text and choose **Cut to bin** (or use its shortcut). The passage moves into `cut/` as its own file, which records the scene it came from and the sentence around it.
- Deleted scenes go into the bin whole.
- The bin is searchable. **Restore** puts a passage back where it came from if that spot still exists, or at the cursor if it doesn't.

### Status labels

The default statuses are idea → draft → revised → done, and you can change them per vault. The outline shows each scene's status and word count, with totals for each chapter and part (e.g. "Ch. 3: 4/6 drafted · 8,240 words").

### Inbox

Each idea from the phone becomes its own file in `inbox/`, with its tags in the header. Because filenames are unique, the phone and the desktop never edit the same file, so these files can't cause sync conflicts. In the desktop's Inbox panel you can turn an idea into a note, add it to an existing note or scene summary, or make it a new scene in the outline. Filed ideas are removed from the inbox.

## 5. Snapshots, history and sync

Snapshots work like Google Docs version history, stored as git commits in the vault.

| When | What happens |
|---|---|
| ~1 s after you stop typing | The file is saved to disk (atomic write) |
| 2 min idle, switching scenes, or closing the app | Automatic snapshot (git commit) of whatever changed |
| Every 10 min of continuous writing | Snapshot even if you haven't paused |
| **Name this version** | Snapshot plus a name (a git tag), shown prominently in history |

All of these timings can be changed in `vault.toml`.

- **History panel** for one scene or a whole project:
  - snapshots are grouped by session and day
  - word-level changes are highlighted
  - you can restore a whole version or copy back just one passage
- Commit messages are written automatically, e.g. "Edited The night market (+312 words)".
- Old snapshots are kept forever because text is tiny. The panel groups them so the list stays readable.
- History is never rewritten, and the app never force-pushes.

### Sync

- **Phase 1:** a plain backup push to GitHub after each snapshot, so your work leaves this machine from day one.
- **Phase 4:** full two-way sync.
  - The app pulls when it starts and every few minutes after.
  - Local snapshots are rebased onto whatever came from GitHub. That's usually phone ideas, which can't conflict.
  - If two computers changed the same scene while offline, both versions are kept side by side and flagged for you to merge. Nothing is overwritten.
- **Move out:** copies a project and its full history into a new private repo (`git subtree split`), then removes it from the vault.

## 6. Timeline

Each project uses **one calendar**. A project that belongs to a world uses the world's calendar.

- **Real**: Gregorian dates and times.
- **Invented**: defined in `world.toml` or `project.toml`, with months and their lengths, eras, and optional leap-year rules and weekday names.

```toml
[calendar]
name = "Reckoning of the Glass Coast"
months = [
  { name = "Thaw", days = 30 },
  { name = "Bloom", days = 31 },
  # …
]
eras = [{ name = "After the Drowning", short = "AD" }]
```

Each scene or event gets a place in time through the `when:` field in its header:

| Kind | Example | Meaning |
|---|---|---|
| Fixed | `when: 1998-03-14 19:00` or `when: 3 Thaw 412 AD` | A point in the calendar |
| Relative | `when: { from: The wedding, offset: +3d }` | Measured from another scene or event. "Day 4" counts from a chosen Day 1 |
| Order only | `when: { after: The harbour }` | Just after, before or between other items |
| Unplaced | *(no `when`)* | Waits in a tray beside the timeline |

**How the resolver in `core` works:**
- Every fixed date becomes a single number (minutes since the calendar's start). Sorting and offsets are then plain arithmetic in any calendar.
- A relative time is worked out by following its chain back to a fixed point.
- Order-only items are placed between their neighbours.
- Contradictions ("X is 3 days after Y but also before Y") and cycles are flagged.

**The timeline view:**
- Shows story order, in lanes by POV character, thread or place.
- Labels each item with its reading-order number, so flashbacks stand out.
- Shows order-only items between their neighbours with dashed spacing.
- Times are edited in the side panel. Order-only items can also be dragged.

## 7. Network

A map of a project's characters, places and plot points, and how they connect. You build it by hand, and it can show any moment in the story.

### What's on it

- **Nodes:** characters, places, plot points and threads (sources and arguments in nonfiction). Scenes stay off the map; plot points stand in for them.
- **Links** (thin, automatic): drawn from `[[links]]` and headers, e.g. a plot point that involves Mara.
- **Relationships** (labelled): lines you draw, like "mentor of", "betrays" or "causes".
- **Scope:** one network per project. World notes and notes from other projects can be added, and are marked with where they come from.

### A plot point

```markdown
---
id: ev_8d1c2e
type: event
title: The ledger leaves port
when: { after: The night market }
involves: [Mara Venn, Old Teodor]
threads: [The missing ledger]
scenes: [night-market]          # where it happens on the page, if anywhere
---

Teodor ships the ledger out disguised as candles.
```

### A relationship

Each relationship is its own small note in `relationships/`, so a relationship between two people doesn't belong to either of them.

```markdown
---
id: rl_4k2m9a
type: relationship
between: [Mara Venn, Old Teodor]
label: trusts
directed: true                  # reads Mara → Teodor; leave out for mutual ones
begins: The harbour             # optional: doesn't exist before this
changes:
  - at: The ledger leaves port
    label: distrusts
ends: The Drowning              # optional
---

She owes him for the berth, and both of them know it.
```

- `begins`, `changes` and `ends` point to plot points or scenes, and take effect at their time in the story.
- A relationship between two world notes can be promoted to the world, so every book in the series shows it.

### Moving through the story

- A slider along the bottom follows story time (the timeline's order).
- At each point, relationships show their state then, plot points still to come are dimmed, and characters are dimmed until they first appear.
- Relationships that point at unplaced items are flagged, since they can't be placed in time.

### Building on it

- Drag nodes to arrange them. Positions are saved in `network.toml`, and new nodes are placed automatically.
- Drag from one node to another to draw a relationship, then type its label (autocompleted from labels used before).
- Double-click empty space to add a plot point.
- Click a node or line to edit it in the side panel.
- Filter by note type or thread, and hide automatic links.

## 8. Desktop app

```
┌─────────────┬────────────────────────────────┬──────────────┐
│ Outline     │                                │ Scene        │
│ Notes       │ Editor / Timeline / Network    │ Links        │
│ Inbox       │                                │ Comments     │
│ Cut bin     │                                │ History      │
└─────────────┴────────────────────────────────┴──────────────┘
```

- **Outline tree**
  - Drag to reorder, or to move items between chapters and parts. Several items can be selected at once.
  - Split a scene at the cursor, or merge it with the next one.
  - Every row shows status and word count.
  - Optional POV and thread columns show where a thread goes quiet.
- **Editor** (Medium-style)
  - Headings, bold, italic, underline (Ctrl+U, stored as `<u>…</u>`), quotes, lists, links, scene breaks and footnotes.
  - Markdown shortcuts as you type: `##`, `>`, `-`, `1.`, and `---` for a scene break (stored as `---`).
  - Shift+Enter in a list starts a sublist under the current item; elsewhere it's a line break.
  - `[[` autocomplete, spellcheck and a live word count.
  - Focus mode hides the side panels.
- **Notes**: the same editor, plus backlinks, appearances, unlinked mentions, and a small graph of what connects to the note.
- **Search**: full text across the vault, filtered by project, status, POV, thread or note type.
- **Command palette** (Ctrl+K) for every action.

### The editor island

ProseMirror runs inside the Leptos UI, with a thin `wasm-bindgen` layer between them:

- **Rust → JS:** `mount(el, markdown, options)`, `set_content`, `focus`, `insert_link`
- **JS → Rust:** `on_change(markdown)` (debounced), `on_link_query(text)` for autocomplete, `on_cut_to_bin(selection)`, `on_split(position)`

ProseMirror owns the text while you type, and Rust owns the files. The editor opens one scene at a time, so documents stay small and fast.

## 9. Phone app

The phone app is a PWA built from `crates/phone` and published to GitHub Pages by GitHub Actions. Added to the home screen, it behaves like an installed app.

- **Setup:** paste the vault repo name and a fine-grained GitHub token, or scan a QR code from the desktop app. The token only has Contents read & write, on the vault repo only. If you lose the phone, revoke the token on GitHub.
- **Capture:** one big text box. Saving queues the idea on the phone and sends it to `inbox/` through GitHub's API. Without a connection, the idea is sent the next time the app opens or the connection comes back.
- **Tagging:** tap to tag the idea with a project, characters, places, threads or scenes. Names autocomplete from `.needle/index.json`, a small list the desktop app keeps current, so the phone never has to crawl the repo.
- **Lookup:** browse and search notes, read-only. Notes are fetched when opened and cached for offline use, and pinned notes stay available offline.

## 10. Sharing and comments

### How a link works

1. Choose what to share: a scene, chapter, part or the whole manuscript. Only prose is shared, never notes.
2. The desktop app renders it, encrypts it with a new random key (AES-256-GCM) and uploads the encrypted copy to the Worker.
3. The link looks like `https://<you>.github.io/needle-and-thread/read#<id>.<key>`. Browsers never send anything after the `#` to a server, so neither GitHub nor Cloudflare can read your text.
4. **Always latest:** after each snapshot that touches the shared part, the app uploads a new version, at most once a minute. Readers see snapshots, never half-typed sentences.

### Comments

- Readers select text and comment under a name they type once, which their device remembers.
- Everyone with the same link sees everyone's comments and can reply.
- Comments, replies and names are encrypted with the link's key before they leave the reader's browser.
- New comments appear in the desktop app's side panel, highlighted on the words they refer to. You can reply and resolve from there too.
- Separate groups (beta readers, an editor) can each get their own link. Each link has its own discussion and can be revoked on its own.

### Keeping comments attached as the text changes

Each comment stores:
- the words it's on, plus a little text either side
- the scene
- the snapshot it was made on

When you edit, the desktop app finds the comment's new position in three steps:
1. Follow the words through the changes between that snapshot and now.
2. If that fails, search for the quoted words.
3. If the words are gone, keep the comment with its original quote, marked "text removed".

Each upload includes the updated positions, so readers always see comments on the right words.

### Author tools

Every share also has a secret **author key**.
- The desktop app's **Open as author** opens the link in your browser with the key included.
- The phone app already has the keys, from the vault.

The author key adds these tools to the reader page: reply as author, resolve, delete comments, and revoke the link.

### Worker

Written in Rust (`workers-rs`). It stores share text in KV and comments in D1, all encrypted, along with hashes of each share's author key and comment token.

| Endpoint | Who can call it |
|---|---|
| `PUT /s/{id}` | Author: upload a new version |
| `GET /s/{id}` | Anyone with the link |
| `GET /s/{id}/comments?since=` | Anyone with the link |
| `POST /s/{id}/comments` | Anyone with the link: requires a token derived from the link key; rate-limited, size-capped |
| `PATCH` / `DELETE /s/{id}/comments/{cid}` | Author: resolve, delete |
| `DELETE /s/{id}` | Author: revoke (deletes the text and its comments) |

## 11. Export

Each project has **compile** settings that control:
- what's included (parts, chapters, statuses)
- the scene separator (`#`, `* * *`, blank line)
- the chapter heading style
- the title page, front matter and back matter

| Format | How | Presets |
|---|---|---|
| PDF | Typst, built into the app as a library | Book (5×8", 6×9"); manuscript |
| EPUB 3 | `epub-builder` | Ebook with a table of contents from the outline, and a cover |
| .docx | `docx-rs` | Standard manuscript format (12 pt, double-spaced, 1" margins, surname/title/page header); plain, for Google Docs or Ellipsus |
| Medium | Clipboard: HTML and Markdown | Only formatting Medium accepts: headings, emphasis, quotes, lists, links |

The app ships its own fonts, so a PDF comes out the same on every machine.

## 12. Security and privacy

- **Your writing on GitHub is private but not encrypted.** GitHub could technically read it, just as with Google Docs. Encrypting the vault would break phone lookup, so this can be revisited if it matters.
- **Shared text and comments are end-to-end encrypted.** Cloudflare only stores scrambled data. Anyone with a link can read and comment, so forwarding a link forwards access. Revoke the link to cut that off.
- **The phone's GitHub token is kept in browser storage on `<you>.github.io`.** Every Pages site under your account shares that storage. Either keep other Pages sites off that domain, or give Needle and Thread its own custom subdomain.
- **Posting a comment requires a token derived from the link key.** Someone who only knows a share's id can't post to it.

## 13. Build plan

| Phase | Scope |
|---|---|
| 0 · Spikes | Short experiments, each answering "will this work?" before anything is built on it: ProseMirror island in Tauri + Leptos with a Markdown round-trip; spellcheck in WebKitGTK; git2 commit and push with a token; a Typst PDF from Rust |
| 1 · Core | Vault and projects, outline tree, editor, notes, links, backlinks and mentions, worlds, status and word counts, cut bin, autosave, snapshots and history, search, backup push to GitHub |
| 2 · Exports | Compile settings, PDF, EPUB, .docx, copy for Medium |
| 3 · Timeline + network | Calendars, resolver, timeline view, plot points, relationships, network map with story-time slider |
| 4 · Sync + phone | Two-way sync, Pages deploy, phone capture, tagging and lookup, inbox filing, QR setup |
| 5 · Sharing | Worker, encryption, reader page, comments and anchoring, author tools, desktop comments panel |
| Later | Windows builds; moving a project out of the vault; a chapter as one continuous document; citations for nonfiction; notes-to-self inside the prose |

## 14. Risks

| Risk | Mitigation |
|---|---|
| Markdown is read by two parsers, ProseMirror (JS) and `core` (Rust), which could disagree | A fixed Markdown subset, with shared round-trip test files run against both in CI |
| WebKitGTK, Tauri's Linux webview, has IME and performance quirks | Phase 0: renders correctly on Hyprland/Wayland with no workarounds; typing in a 10k-word scene was fine. IME is untested. Its spellchecker only checks newly typed text, so the app has its own (see §2) |
| Comment anchoring goes wrong after heavy rewrites | Follow snapshot changes first, then search for the quote, and fall back honestly to "text removed" |
| Good book typography in Typst takes iteration | Start with one solid template per preset |
| The Pages origin is shared by all your Pages sites | Use a custom subdomain if you publish other sites |

## 15. Open questions

- What should the vault repo be called?
- Are 2 min idle / 10 min max the right snapshot timings?
- Should share links leave out scenes still marked *idea*?
- Should you be able to pause a link's updates during a big rewrite?
- Do you want notes-to-self inside the prose that never appear in exports?
- Should the Pages site use a custom domain?
- Network canvas: draw it in Rust (Leptos + SVG), or use a JavaScript graph library as a second island like the editor? Decide with a short spike before building it.
