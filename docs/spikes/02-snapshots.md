# Spike 2 — Snapshots

*2026-10-03 · git2 0.21 linked to the system libgit2 1.9.7, similar 3.2*

## Answers

**Can Rust snapshot the vault in the Google Docs style?** Yes.
- **When:** `needle_vcs::Scheduler` snapshots after a 2-minute pause, or every 10 minutes during writing that never pauses. Switching scenes and quitting snapshot right away.
- **What:** each snapshot is a commit of the whole vault, with a generated message ("Edited night-market (+4 words)").
- **Cost:** in a 300-scene, million-word vault, about 2 ms after editing one scene, and 22 ms for the very first snapshot.

**History, compare and restore?** Yes.
- **History:** loading one scene's history takes 0.2 ms.
- **Compare:** a word-level diff (`needle_core::diff`, using similar) shows what changed since each version.
- **Restore:** puts the old text back but keeps the current front matter. The text being replaced is snapshotted first.
- **Named versions:** annotated tags, `refs/tags/versions/<commit>`.

**Push to a private GitHub repo with a token?** Yes, over HTTPS (`x-access-token`).
- **Speed:** about 1.9 s per push.
- **Bad token:** fails in under a second with a clear message, with no retry loop.
- **No overwrites:** a push is refused when GitHub has snapshots the vault doesn't. This was tested by adding an inbox file through GitHub's API, the way the phone will.

## Other findings

- **No C build:** `git2` links the system libgit2, so nothing compiles from C. Windows builds will need its vendored build instead.
- **Author:** snapshots use your git identity, falling back to "Needle and Thread".
- **Word counts:** history uses the same count as the editor (`count_markdown_words`): a wikilink counts its label, and link URLs and `<u>` tags don't count.
- **Test repo:** `alasdiel/needle-and-thread-test-vault` (private), kept for phase 4.
- **Try it:** `crates/vcs/examples/push.rs` pushes any vault (`NEEDLE_GITHUB_TOKEN=… cargo run -p needle-vcs --example push -- <vault> <url>`).

## For later phases

- **Sync (phase 4):** pull and rebase before pushing, and replace libgit2's raw rejection message with a plain one.
- **Snapshot timings:** whether 2 min / 10 min is right is still open. `NEEDLE_SNAPSHOT_IDLE_SECS` and `NEEDLE_SNAPSHOT_MAX_SECS` override them for testing.
- **History panel:** long unchanged stretches are shortened. A "next change" button would help in long scenes.
