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
| Network map | Drawn as a bulletin board strung with red thread: cork, each kind of note a different pinned paper (polaroid, postcard, index card, folder), zones you drag out and name, and Write and Marker tools for marks you add yourself. A tool for thinking, not for an audience (§7) |
| Network map drawing | In Rust, Leptos + SVG, not a JavaScript graph library; panned and zoomed as one CSS-transformed layer (spike 3) |
| Plot points | One note type with events (backstory included); can sit on the timeline |
| Devices | A Linux desktop and a phone, both first-class: the same writing, notes, reading and restructuring on each (§9). **Windows is a release target now, not "later"** — other people are meant to run the app, which they weren't before |
| Packaging | GitHub Actions. CI runs the tests, clippy and the editor's typecheck on every push; a `v*` tag builds the packages and leaves a **draft** release to publish by hand. Linux ships a `.deb` and an AppImage; Windows an MSI and a setup `.exe` (NSIS), unsigned for now. **Everything the app needs comes with it**, rather than being a system package the reader has to install first: the Hunspell dictionary built into the binary, SQLite and libgit2/libssh2 compiled in already. The one exception is the webview, which can't be bundled — WebKitGTK on Linux, WebView2 on Windows |
| Desktop app | Tauri 2 + Leptos, in Rust |
| Editor | ProseMirror as a small JavaScript island (Rust has no mature rich-text editor) |
| Editor feel | Like Medium: formatting shows as formatting, no visible Markdown symbols |
| Look | A sewing-pattern bench. Each scene is drawn as a pattern piece (cutting line, stitching line, notches); cut lines, seams and stitches mark Split, Merge and scene breaks. Each note is a fabric swatch with pinked edges, pinned to the bench, with a smaller swatch beside it listing what links to it. A scene's POV, cast, places and threads are on its envelope (like the back of a pattern envelope listing notions), beside the scene and staying in view as it scrolls, or a tab on the page's edge when the window is narrow. A link to a note that doesn't exist yet is basted (long loose stitches). The motifs stay in the app's frame, never in the text |
| Colours | From Gwen (League of Legends): aqua threads, saturated blues, violet. Dark, a Shadow Isles cutting mat, is the main look; light is pattern tissue. System, Light or Dark is chosen per computer |
| Fonts | Bundled, all OFL: Literata for the text, Fraunces (soft and wonky) for titles, Alegreya SC for labels, Alegreya Sans for controls, Caveat for marks you handwrite on the network board (§7) |
| Typography as you type | Four separate settings, each explained in the app and on by default: curly double quotes, curly single quotes/apostrophes, `--` → em dash, `...` → ellipsis. They never rewrite existing text |
| Spellcheck | Our own, not the webview's: spellbook (Rust, Hunspell-compatible) with a bundled US English (`en_US`) dictionary — shipped with the app, since an AppImage or a Windows install can't rely on a system `hunspell-en-us`. Underlines everything as soon as a scene opens, accepts names used in `[[links]]`, right-click for suggestions or "Add to dictionary" (`.needle/dictionary.txt`) |
| Phone app | PWA on GitHub Pages, sharing Rust UI code with the desktop |
| Storage & sync | One private GitHub "vault" repo for all writing; any project can be moved out to its own repo later |
| Vault location | Chosen with a folder picker on first launch; the app remembers it |
| File headers | TOML between `+++` lines, edited in place so untouched fields keep their formatting |
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
│   ├── vault/        reads and writes the vault: projects, outline, scenes, notes, cut bin
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

`crates/desktop` follows the layout of the official create-tauri-app Leptos template, so the Tauri and Leptos docs apply as written. Crates are added as their phase starts (`index`, `export`, `ui`, `phone` and `worker` don't exist yet).

The search index is a SQLite file in the app's cache folder, one per vault, never inside the vault. Before each search it stats every file and re-reads only those that changed, so it can't fall behind, and if it's lost it's rebuilt from the files. (Backlinks and mentions still read the files directly; they can move to the index if a vault ever gets big enough to need it.)

`core` is used by the desktop app, the phone app and the reader page. Parsing, link resolution, timeline maths and encryption therefore behave the same everywhere. The Worker reuses its API types.

## 4. The vault

### Layout

```
vault/
├── .needle/
│   ├── vault.toml               settings: status labels, snapshot timing
│   ├── shares.toml              share registry: ids, scopes, keys
│   ├── templates/               one per note type (character.md…), yours to edit
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

Reading order lives in `outline.toml`, a list of chapters. A part is a label on chapters: consecutive chapters with the same `part` form it. A scene file keeps the name it was created with, even after you retitle it or move it to another chapter. The app rewrites this file whenever the outline changes.

```toml
[[chapter]]
id = "ol_a1rr4v4l2x"
title = "Arrival"
part = "Part One: Landfall"
summary = "Mara reaches Tidewater and hears about the ledger."
scenes = ["the-harbor", "night-market"]
```

This way, moving a scene changes one line of the outline instead of renaming a chain of files. Because a scene's path never changes, its history, links and anchored comments stay attached to it.

### A scene

```markdown
+++
id = "sc_7f3k9qa2mx"       # stable id, survives any rename
title = "The night market"
status = "draft"           # idea | draft | revised | done
summary = "Mara trades the compass and learns the ledger has left port."
pov = "Mara Venn"
cast = ["Mara Venn", "Old Teodor"]
places = ["Night Market"]
threads = ["The missing ledger"]
when = { from = "The harbor", offset = "+6h" }
+++

The market opened at dusk, as it always had…
```

The header is TOML between `+++` lines. When the app changes a field, everything else in the header (formatting, comments, fields it doesn't know) stays exactly as written. `+++` also can't be confused with a `---` scene break.

The body holds only prose, which is exactly what gets exported. Who appears in the scene, where it happens and which threads it advances all go in the header, so the prose never fills up with link syntax. The app also notices characters, places and threads the text links to or mentions (by title or alias) that the header doesn't list, and offers them as faint chips on the scene's envelope; nothing is added without a click.

### A note

```markdown
+++
id = "nt_2m8x1a9kqe"
type = "character"
title = "Mara Venn"
aliases = ["Mara", "the Captain"]
+++

Harbor pilot, thirty-four. Lost her ship in [[The Drowning]]…
```

Ids start with `sc_` for scenes, `nt_` for notes and `ol_` for chapters.

Note types:
- **character**
- **place**
- **thread**: a plot thread, or a line of argument in nonfiction
- **source**: author, title, year, URL, pages, quotes
- **event**: anything that happens, backstory included; can sit on the timeline and the network
- **relationship**: a named line between two notes on the network (§7)
- **note**: anything else

Each type has its own template: an ordinary note in `.needle/templates/` (`character.md`, `place.md`…) whose header fields and text every new note of that type starts with. The app writes the defaults the first time you make a note, and fills in `id`, `type` and `title` itself. Labels depend on the project's kind: fiction uses Scene, Thread and Plot point; nonfiction uses Section, Argument and Event.

### Links and shared worlds

- `[[Name]]` or `[[Name|shown text]]` links to any note, and typing `[[` brings up autocomplete.
- A name is looked up in the project first, then in the project's world. Self-contained projects have no world.
- In each, titles come before aliases, so `[[Mara]]` reaches "Mara Venn" through her alias. Case and apostrophe style don't matter. A name that two notes share at the same level (an alias used twice, say) is flagged rather than guessed.
- Autocomplete always writes the title, with what you typed as the shown text (`[[Mara Venn|Mara]]`), so links keep working when an alias changes.
- A note in another project is linked with its project's folder name: `[[tidewater/Mara Venn]]`.
- Renaming a note updates every link and header that names it by its title. A link keeps the words it shows (`[[Old Teodor]]` becomes `[[Teodor Brask|Old Teodor]]`), so renaming never changes the prose. The note's file keeps its name.
- Each note shows three lists:
  - **backlinks**: what links to it
  - **appearances**: scenes that list it in their header
  - **unlinked mentions**: scenes that use its name or aliases (matching case, since names are proper nouns)
- A project can belong to one **world**. The world holds the characters, places, history and calendar that a series shares.
- **Promote to world** (the note's ⋯ menu) moves a note from a project into its world, for example when book two starts. A project's world is chosen in Project settings (the ⋯ by its title), which can also make a new world.
- For nonfiction, a world can serve as a shared research library.

### Cut bin

- Select text and choose **Cut to bin** from the right-click menu, or press Ctrl+Shift+X. The passage moves into `cut/` as its own file, which records the scene it came from and the sentence around it. A note at the foot of the page offers Undo.
- Deleted scenes go into the bin whole, with `cut_at`, `cut_from_scene`, `cut_from_chapter` and `cut_after_scene` (the scene before it, empty if it was first) added to their header. Merging two scenes bins the second one, so its header survives. Deleted notes go in whole too, with `cut_at` and `cut_from_note`.
- The basket at the foot of the sidebar opens the bin as a panel on the right, newest first and grouped by day. A passage shows its first lines; a scene or note shows its title.
- **Restore** puts a passage back where it came from if that spot still exists, or at the cursor if it doesn't. A scene goes back after the scene it followed (or first in its chapter), else at the end of its chapter, else among the scenes the outline doesn't place. A note goes back to its old path, or beside it if a new note has taken the name.
- The bin is searchable: what's in it comes last in the results, under "Cut bin" (a passage under its scene's title), and the Cut bin filter shows only that. Opening one shows it in the bin, marked.
- Undo and Redo keep the bin in step: Ctrl+Z right after a cut puts the passage back and takes it out of the bin, Redo bins it again, and Ctrl+Z after a Restore puts it back in the bin.

A passage's file:

```markdown
+++
cut_at = "2026-10-07T14:20:00Z"
cut_from_scene = "night-market"
cut_from_title = "The night market"
text_before = "Mara walked the length of it twice. "
text_after = " She kept the compass wrapped in her sleeve."
starts_paragraph = false
ends_paragraph = false
+++

The smell of the harbor came in under everything: tar, salt, the sweet rot of fruit nobody had sold.
```

Restore looks for `text_before` and `text_after` side by side (in one paragraph, or in neighbouring ones when the passage began or ended a paragraph), ignoring differences in spacing. If they're found exactly once, that's the spot.

### Status labels

The default statuses are idea → draft → revised → done, and you can change them per vault. The outline shows each scene's status and word count, with totals for each chapter and part (e.g. "Ch. 3: 4/6 drafted · 8,240 words").

Each status is drawn as a ring that fills as a scene moves along the list: empty for the first status, full for the last. Any number of statuses works, and there are no colours to tell apart. A status that isn't in the list is drawn dashed.

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
  - a session is snapshots less than 30 minutes apart, within one day. Each folds into one row ("13:05 – 14:32 · 4 snapshots · +31 words"); the latest is open, and named versions show even in a closed one
  - each snapshot shows the words it added or took out from this file, not the vault-wide message
  - **Put back:** click a removed passage in a version's changes, then Put back. It goes back where it was, as one edit that Undo takes back. If something replaced it, both stay, the old passage first, so nothing is lost. Words rewritten together show as one removal and one addition, so a rewritten sentence goes back whole
- Commit messages are written automatically, e.g. "Edited The night market (+312 words)".
- Old snapshots are kept forever because text is tiny. The panel groups them so the list stays readable.
- History is never rewritten, and the app never force-pushes.

### Sync

- **Phase 1:** a plain backup push to GitHub after each snapshot, so your work leaves this machine from day one.
  - Set up in Settings › Backup: the repository's SSH address (`git@github.com:you/novel.git`) and an **After each snapshot** switch, both kept in `vault.toml` under `[backup]`. **Back up now** pushes at once, and the vault also backs up when it opens.
  - It pushes over SSH with your own keys, built in (libgit2 with libssh2): those in ssh-agent, then `~/.ssh/id_ed25519`, `id_ecdsa` or `id_rsa` without a passphrase. The server's key must already be in `~/.ssh/known_hosts` (connect once with `ssh -T git@github.com`). `~/.ssh/config` isn't read.
  - Beside "Saved": "· backed up", "· backing up…", or "· not backed up" in red, with the reason on hover; a click opens the settings. Pushes never force, so a repository with history this vault lacks is refused and left as it is.
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

It's drawn as a **bulletin board**, the kind strung with red thread. This is a tool for your own thinking, not something to show an audience, so it favours being expressive over staying tidy: nothing in it exists to keep the board neat.

### What's on it

- **Nodes:** characters, places, plot points and threads (sources and arguments in nonfiction). Scenes stay off the map; plot points stand in for them.
- **Links** (thin, automatic): drawn from `[[links]]` and headers, e.g. a plot point that involves Mara.
- **Relationships** (labelled): lines you draw, like "mentor of", "betrays" or "causes".
- **Zones:** sheets of kraft paper pinned under a group of cards, which you drag out and name yourself. They mean nothing to the rest of the app: a zone is only how you've arranged your own thinking. Saved in `network.toml`.
- **Marks:** handwriting and marker you add yourself (see "Marks you make" below).
- **Scope:** one network per project. World notes and notes from other projects can be added, and are marked with where they come from.

### How it's drawn

The board is cork, stained in the dark theme. Each kind of note is a different piece of pinned paper, so kinds are told apart by shape before colour:

| Kind | Pinned up as |
|---|---|
| Character | A polaroid |
| Place | A postcard |
| Plot point | A ruled index card |
| Thread | A manila folder with a tab |

- **Pins** take the kind's colour, the same four already used across the app.
- **Relationships** are red string from pin to pin, sagging a little, labelled on a strip of masking tape.
- **Automatic links** are thinner, pale twine.
- **Cards tilt** a degree or two, never more, so a big board still reads.
- **Everything the app writes is in the app's own type**, titles included. Handwriting is only ever something you added.

The map is drawn in Rust, with Leptos and SVG, and panned and zoomed as one CSS-transformed layer (spike 3).

### Marks you make

Two tools, for thinking on the board itself:

- **Write:** a handwritten note (Caveat), put anywhere on the board and dragged about. Loose on the board by default; dropped onto a card, it travels with that card.
- **Marker:** drag around a group to circle it, or between two cards for an arrow. Marks sit above the strings, and each is rubbed out on its own.

Marks are the only handwriting on the board, because you wrote them. They're kept in `network.toml` alongside the node positions.

### `network.toml`

One per project, holding only how the board is arranged. What's *on* the board comes from the notes themselves, so this file never has to be repaired: a card with no entry has simply never been placed, and an entry for a note that's gone is ignored.

```toml
[nodes]
nt_8d1c2e5fqa = { at = [120.0, 240.0], turn = -1.5 }

[[zones]]
id = "zn_4k2m9a7x1q"
name = "The harbour"
at = [0.0, 0.0]
size = [520.0, 380.0]

[[marks]]
kind = "note"                  # handwritten; "ring" and "arrow" are marker
id = "mk_7f3k9qa2mx"
text = "who has it now?"
at = [600.0, 120.0]
turn = -6.0
on = "nt_8d1c2e5fqa"           # dropped on a card: `at` is measured from it, and it travels with it
```

`turn` is a tilt in degrees, and is left out when it's zero.

### A plot point

```markdown
+++
id = "nt_8d1c2e5fqa"
type = "event"
title = "The ledger leaves port"
when = { after = "The night market" }
involves = ["Mara Venn", "Old Teodor"]
threads = ["The missing ledger"]
scenes = ["night-market"]       # where it happens on the page, if anywhere
+++

Teodor ships the ledger out disguised as candles.
```

### A relationship

Each relationship is its own small note in `relationships/`, so a relationship between two people doesn't belong to either of them.

```markdown
+++
id = "nt_4k2m9a7x1q"
type = "relationship"
between = ["Mara Venn", "Old Teodor"]
label = "trusts"
directed = true                 # reads Mara → Teodor; leave out for mutual ones
begins = "The harbor"           # optional: doesn't exist before this
ends = "The Drowning"           # optional
changes = [
  { at = "The ledger leaves port", label = "distrusts" },
]
+++

She owes him for the berth, and both of them know it.
```

- `begins`, `changes` and `ends` point to plot points or scenes, and take effect at their time in the story.
- A relationship between two world notes can be promoted to the world, so every book in the series shows it.

### Moving through the story

- A slider along the bottom follows story time (the timeline's order).
- At each point, relationships show their state then, plot points still to come are dimmed, and characters are dimmed until they first appear.
- Relationships that point at unplaced items are flagged, since they can't be placed in time.

### Building on it

A toolbar over the cork: **Move**, **String**, **Write**, **Marker** and **Zone**, then **Pin up** and **Show**. A tool stays chosen until another is picked or Escape is pressed.

- **Move:** drag cards to arrange them. Positions are saved in `network.toml`, and new cards are placed automatically, near what they link to.
- **String:** drag from one card to another to tie them, then type its label on the tape; labels used before are offered as you type. It's a new note in `relationships/`, titled after its two ends ("Mara Venn and Old Teodor"), reading from the first card to the second. Let go anywhere but on a card, or press Escape, and nothing is tied. A card from another project is named `project/Title` in `between`, so the name always finds it again.
- **Double-click bare cork** to add a plot point: a blank index card appears there for its title. Escape, or leaving it empty, makes nothing.
- **Click a card or a string** to see it in the side panel; **double-click a card** to open its note.
  - A string's panel edits its label, makes it one way or both ways, turns it round, lists how it changes along the way (`begins`, `changes`, `ends`), opens its note, and can move it to the cut bin.
  - A card's panel lists its strings (each opens in the panel) and opens the note. A card pinned up by hand can be taken down there; the note itself stays.
- **Zone:** drag out a sheet of kraft paper, then name it on its tape. The tape is the sheet's handle: drag it to move the sheet, double-click it to rename. Pressing the paper itself pans the board, so a board covered in zones can still be moved around. A chosen zone has a corner to resize it, and Delete (or Take down) removes it. Cards on it don't move with it.
- **Pin up:** a list of notes that aren't on the board: other projects', the world's, and this project's plain notes and sources (pinned up as a plain sheet). Picking one pins it in the middle of the view.
- **Show:** leave kinds of card off, show only one thread and what's tied or linked to it, and hide automatic links. It's a way of looking, so it isn't saved.

Until the story-time slider (M6), the board shows each relationship as it stands at the end of the book: its last label, and slack and faded if it `ends`. The side panel has what came before.

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
  - Split a scene at the cursor: a cut line shows where, and Enter confirms. Or merge it with the next one: a seam marks the join, then fades.
  - Every row shows status and word count.
  - Optional POV and thread columns show where a thread goes quiet.
- **Editor** (Medium-style)
  - Headings, bold, italic, underline (Ctrl+U, stored as `<u>…</u>`), quotes, lists, links, scene breaks and footnotes.
  - Markdown shortcuts as you type: `##`, `>`, `-`, `1.`, and `---` for a scene break (stored as `---`, shown as three stitches).
  - Shift+Enter in a list starts a sublist under the current item; elsewhere it's a line break.
  - `[[` suggests notes by title or alias as you type; Enter or Tab picks one.
  - Clicking a link opens its note, offers to make one if there isn't one yet, or lets you choose between notes that share the name. Ctrl-click opens the note straight away.
  - Spellcheck (which accepts the names in your notes' titles and aliases) and a live word count.
  - Focus mode hides the side panels.
- **Notes**, in the sidebar's Notes tab, grouped by type: the same editor, on a swatch, with the title and aliases at the top. Beside it (below it in a narrow window) a smaller swatch lists the scenes that name the note in their header, the scenes and notes that link to it, and unlinked mentions, each with a **Link** button. The small graph of what connects to a note waits for the network view (phase 3).
- **Search**: full text across the vault, filtered by project (and its world, or the whole vault), scenes or notes, status, POV or thread. Each word typed matches the start of a word, "quoted words" match exactly, and accents don't matter; a title match ranks first. Two places, one query:
  - **Ctrl+K** opens a box over the page; ↑↓ and Enter open a result, Ctrl+Enter keeps the list in the sidebar.
  - The sidebar's **Search** tab keeps the results listed while you open them one by one.
  - A result opens with the words highlighted and the first one in view.
- **Command palette** (Ctrl+K) for every action.

### The editor island

ProseMirror runs inside the Leptos UI, with a thin `wasm-bindgen` layer between them:

- **Rust → JS:** `mount(el, markdown, options)`, `set_content`, `focus`, `insert_link`
- **JS → Rust:** `on_change(markdown)` (debounced), `on_link_query(text)` for autocomplete, `on_cut_to_bin(selection)`, `on_split(position)`

ProseMirror owns the text while you type, and Rust owns the files. The editor opens one scene at a time, so documents stay small and fast.

## 9. Phone app

**A full client, not a capture box.** Writing happens on a phone as much as at a desk, so the phone does the same work: write prose, make notes, read and search, and restructure. It isn't a cut-down companion to the desktop; it's the same vault through a smaller window.

It's a PWA built from `crates/phone` and published to GitHub Pages by GitHub Actions. Added to the home screen, it behaves like an installed app.

### How it reaches the vault

The vault is already a private GitHub repo (§5), so the phone talks to GitHub directly and needs no server of our own. Sharing (§10) is the only part that needs the Worker.

- **Setup:** paste the vault repo name and a fine-grained GitHub token, or scan a QR code from the desktop app. The token has Contents read & write on the vault repo only, and nothing else. If you lose the phone, revoke the token on GitHub.
- **Reading** a file is one API call. The file list comes from the repo's tree, so opening the app costs one call, not one per file.
- **Writing** a file sends back the SHA it was read at. GitHub refuses a stale SHA, so a file changed elsewhere can never be silently overwritten: the app re-reads it, merges, and asks only when it can't.
- **Several files at once** (moving a scene between chapters touches `outline.toml` and nothing else; a rename touches every file that links to it) go as one commit through the Git Data API, so the vault is never left half-changed.

### Sharing code with the desktop

`needle-core` already compiles to wasm and holds everything that isn't I/O: headers, the outline, links and names, diff, word counts. Both apps use it.

`needle-vault` is built on `std::fs` and can't be. So the vault operations the two apps share sit behind a **store trait** — read a file, write a file, write several, list — with a Tauri implementation over the file system and a phone implementation over the GitHub API. The screens are then the same Leptos components on both.

The editor is the same ProseMirror island; it already runs in a webview, and a mobile browser is no different. Touch selection and the on-screen keyboard are the parts that need real work.

### What it does

- **Write:** open a scene or note and write, in the same editor, saving as you pause. Snapshots are the commits it makes.
- **Capture:** one big text box for an idea, filed into `inbox/`. Queued on the phone when offline and sent when the connection returns.
- **Read and search:** the outline, notes and links. Names and titles come from `.needle/index.json`, a small list the desktop keeps current, so the phone never crawls the repo; full text search needs the file contents and is fetched as needed.
- **Restructure:** move scenes between chapters, change status, retitle.

### Offline

The first version needs a connection. After that: a service worker caches what's been opened, writes queue while offline and go up when the connection returns, and pinned notes and scenes are kept for reading.

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

### Citation machine (later)

Requested 2026-10-03; not needed for the first phases.

- **Make a source:** paste a URL, DOI or ISBN and the details are filled in (DOI via Crossref, ISBN via Open Library, URL from the page's metadata), or type them by hand. The result is a normal `source` note.
- **Cite:** insert `[@source, p. 42]` while writing, chosen from your sources by autocomplete.
- **Styles:** APA, MLA, Chicago and any other CSL style, for in-text citations, footnotes and a generated bibliography. Exports format them; the editor shows a preview.
- **Likely tool:** hayagriva, the Rust bibliography library Typst uses, which reads CSL styles.

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
| 3 · Timeline + network | Calendars, resolver, timeline view, plot points, relationships, the network board (zones, marks), story-time slider with the timeline |
| 4 · Sync + phone | Phone reading first (no sync risk: it only reads), then two-way sync on the desktop, then writing, capture and restructuring from the phone; Pages deploy, inbox filing, QR setup |
| 5 · Sharing | Worker, encryption, reader page, comments and anchoring, author tools, desktop comments panel |
| Later | Moving a project out of the vault; a chapter as one continuous document; citation machine for nonfiction (§11); notes-to-self inside the prose |

**Windows builds** are a high priority, taken on their own rather than inside a phase, because other people are meant to run the app. In order:

1. ~~**Bundle the Hunspell dictionary**~~ **done:** `/dictionaries` holds SCOWL's `en_US` (from Debian's `hunspell-en-us`, with its licence), built into the backend with `include_str!` rather than shipped as a Tauri resource, so `scripts/install.sh`'s lone binary has it too.
2. ~~**Build on a `windows-latest` runner**~~ **done:** `release.yml` builds an MSI and an NSIS installer on `windows-latest` beside the Linux packages, and a last job drafts the release from both. CI runs the Rust tests and clippy on Windows as well. SQLite (`rusqlite` bundled) and libgit2/libssh2 (`git2` vendored) compile from source, so they need no system packages. `.gitattributes` keeps LF line endings on Windows checkouts.
3. **Check what WebView2 does differently.** This is the real work: the Linux webview is WebKitGTK and Windows substitutes Chromium-based WebView2, which can't be bundled away. `light-dark()`, container queries and CSS masks are all in current Chromium, so the styling should hold, but it has to be looked at rather than assumed.
4. **Keys for backup.** ~~`ssh_key_files` finds keys through `HOME`~~ fixed: it uses `std::env::home_dir`, which is `USERPROFILE` on Windows. Still unchecked on a real Windows machine: ssh-agent is a service there rather than a socket, and libgit2 looks for `known_hosts` in its own way. The app's own folders are fine: they come from Tauri's `app_data_dir`/`app_cache_dir`, which are already per-platform.
5. **Unsigned installers** warn on first run (SmartScreen). A certificate costs money; decide whether to buy one or tell readers to click through.

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
