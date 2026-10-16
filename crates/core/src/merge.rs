//! Three-way merging by paragraph, for a text changed on two devices since they last agreed
//! (DESIGN §5, Sync). A paragraph changed on one side only takes that side's version; one both
//! sides changed differently is a clash, which keeps this side's version in the text until the
//! writer settles it.
//!
//! A clash waiting to be settled is kept as two texts beside the file (see [`CONFLICTS`]): the
//! other side's version and the version both started from, each equal to the merged text
//! everywhere but the clashes. Comparing the file with those two finds the clashes again however
//! the file has been edited since, and settling one writes the chosen text into all three.

use std::ops::Range;

use similar::{Algorithm, DiffOp, capture_diff_slices};

/// Where the other side's version of a clashing file goes, under a folder named for when it was
/// made: `.needle/conflicts/2026-10-10-1016/<path>`.
pub const CONFLICTS: &str = ".needle/conflicts";
/// Inside that folder, where the version both sides started from goes: `.base/<path>`.
pub const BASE: &str = ".base";

/// The paragraphs of a Markdown text: runs of lines between blank lines, without line endings
/// at their ends.
pub fn paragraphs(text: &str) -> Vec<String> {
    let mut paragraphs = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                paragraphs.push(current.join("\n"));
                current.clear();
            }
        } else {
            current.push(line.trim_end_matches('\r'));
        }
    }
    if !current.is_empty() {
        paragraphs.push(current.join("\n"));
    }
    paragraphs
}

/// Paragraphs back into a text, a blank line between each and a newline at the end.
pub fn join<S: AsRef<str>>(paragraphs: &[S]) -> String {
    let mut text = paragraphs.iter().map(AsRef::as_ref).collect::<Vec<_>>().join("\n\n");
    if !text.is_empty() {
        text.push('\n');
    }
    text
}

/// The result of merging two versions of a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merged {
    /// Both sides' changes, with this side's version wherever they clash.
    pub text: String,
    /// What to keep as the version both started from: the merged text, but the original at
    /// each clash.
    pub base: String,
    /// What to keep as the other side's version: the merged text, but theirs at each clash.
    pub theirs: String,
    /// How many clashes there are.
    pub clashes: usize,
}

/// Merges `mine` and `theirs`, two versions of `base`.
pub fn merge(base: &str, mine: &str, theirs: &str) -> Merged {
    let (b, m, t) = (paragraphs(base), paragraphs(mine), paragraphs(theirs));
    let mut merged = Merged { text: String::new(), base: String::new(), theirs: String::new(), clashes: 0 };
    let (mut text, mut kept_base, mut kept_theirs) = (Vec::new(), Vec::new(), Vec::new());
    for chunk in chunks(&b, &m, &t) {
        match chunk.kind {
            Kind::Clash => {
                merged.clashes += 1;
                text.extend_from_slice(&m[chunk.mine]);
                kept_base.extend_from_slice(&b[chunk.base]);
                kept_theirs.extend_from_slice(&t[chunk.theirs]);
            }
            kind => {
                let taken = match kind {
                    Kind::Theirs => &t[chunk.theirs],
                    _ => &m[chunk.mine],
                };
                text.extend_from_slice(taken);
                kept_base.extend_from_slice(taken);
                kept_theirs.extend_from_slice(taken);
            }
        }
    }
    merged.text = join(&text);
    merged.base = join(&kept_base);
    merged.theirs = join(&kept_theirs);
    merged
}

/// A place where both sides changed the same paragraphs differently. The ranges count
/// paragraphs (see [`paragraphs`]) in each of the three texts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clash {
    pub mine: Range<usize>,
    pub base: Range<usize>,
    pub theirs: Range<usize>,
    /// This side's paragraphs there; empty if it removed them.
    pub mine_text: Vec<String>,
    /// The other side's; empty if it removed them.
    pub theirs_text: Vec<String>,
}

/// Where `mine` and `theirs` still clash, in order through `mine`.
pub fn clashes(base: &str, mine: &str, theirs: &str) -> Vec<Clash> {
    let (b, m, t) = (paragraphs(base), paragraphs(mine), paragraphs(theirs));
    chunks(&b, &m, &t)
        .into_iter()
        .filter(|chunk| chunk.kind == Kind::Clash)
        .map(|chunk| Clash {
            mine_text: m[chunk.mine.clone()].to_vec(),
            theirs_text: t[chunk.theirs.clone()].to_vec(),
            mine: chunk.mine,
            base: chunk.base,
            theirs: chunk.theirs,
        })
        .collect()
}

/// How a clash is settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// The other side's version.
    Theirs,
    /// This side's, then the other side's after it.
    Both,
    Mine,
}

/// The three texts once `clash` is settled by `choice`: this side's, the base and the other
/// side's, each with the chosen paragraphs in the clash's place, so it no longer clashes and a
/// later edit there counts as this side's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settled {
    pub mine: String,
    pub base: String,
    pub theirs: String,
}

pub fn settle(base: &str, mine: &str, theirs: &str, clash: &Clash, choice: Choice) -> Settled {
    let chosen: Vec<String> = match choice {
        Choice::Theirs => clash.theirs_text.clone(),
        Choice::Mine => clash.mine_text.clone(),
        Choice::Both => clash.mine_text.iter().chain(&clash.theirs_text).cloned().collect(),
    };
    let put = |text: &str, range: &Range<usize>| {
        let mut paragraphs = paragraphs(text);
        let range = range.start.min(paragraphs.len())..range.end.min(paragraphs.len());
        paragraphs.splice(range, chosen.iter().cloned());
        join(&paragraphs)
    };
    Settled { mine: put(mine, &clash.mine), base: put(base, &clash.base), theirs: put(theirs, &clash.theirs) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Neither side changed it.
    Same,
    /// Only this side changed it, or both made the same change.
    Mine,
    Theirs,
    Clash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Chunk {
    kind: Kind,
    base: Range<usize>,
    mine: Range<usize>,
    theirs: Range<usize>,
}

/// The texts cut into runs that line up across all three (diff3): stretches neither side
/// changed, and between them what each side has instead.
fn chunks(base: &[String], mine: &[String], theirs: &[String]) -> Vec<Chunk> {
    let in_mine = matches(base, mine);
    let in_theirs = matches(base, theirs);
    let (mut i, mut j, mut k) = (0, 0, 0);
    let mut chunks = Vec::new();
    loop {
        // Paragraphs unchanged on both sides, from here.
        let mut same = 0;
        while i + same < base.len() && in_mine[i + same] == Some(j + same) && in_theirs[i + same] == Some(k + same) {
            same += 1;
        }
        if same > 0 {
            chunks.push(Chunk { kind: Kind::Same, base: i..i + same, mine: j..j + same, theirs: k..k + same });
            (i, j, k) = (i + same, j + same, k + same);
            continue;
        }
        // Up to the next paragraph both sides still have.
        let next = (i..base.len()).find(|&n| in_mine[n].is_some() && in_theirs[n].is_some());
        let (to_base, to_mine, to_theirs) = match next {
            Some(n) => (n, in_mine[n].unwrap(), in_theirs[n].unwrap()),
            None => (base.len(), mine.len(), theirs.len()),
        };
        if (i, j, k) == (to_base, to_mine, to_theirs) {
            break;
        }
        changed(&mut chunks, base, mine, theirs, i..to_base, j..to_mine, k..to_theirs);
        (i, j, k) = (to_base, to_mine, to_theirs);
    }
    chunks
}

/// Sorts out a run where at least one side differs from the base. When all three have the same
/// number of paragraphs, it's taken a paragraph at a time, so neighbouring paragraphs changed
/// on different sides don't clash.
fn changed(chunks: &mut Vec<Chunk>, base: &[String], mine: &[String], theirs: &[String], b: Range<usize>, m: Range<usize>, t: Range<usize>) {
    let kind = |b: Range<usize>, m: Range<usize>, t: Range<usize>| {
        if base[b.clone()] == mine[m.clone()] {
            if base[b] == theirs[t] { Kind::Same } else { Kind::Theirs }
        } else if base[b] == theirs[t.clone()] || mine[m] == theirs[t] {
            Kind::Mine
        } else {
            Kind::Clash
        }
    };
    if b.len() == m.len() && b.len() == t.len() && kind(b.clone(), m.clone(), t.clone()) == Kind::Clash {
        for n in 0..b.len() {
            let (b, m, t) = (b.start + n..b.start + n + 1, m.start + n..m.start + n + 1, t.start + n..t.start + n + 1);
            let kind = kind(b.clone(), m.clone(), t.clone());
            match chunks.last_mut() {
                // Neighbouring clashes are one, to settle together.
                Some(last) if kind == Kind::Clash && last.kind == Kind::Clash && last.mine.end == m.start => {
                    (last.base.end, last.mine.end, last.theirs.end) = (b.end, m.end, t.end);
                }
                _ => chunks.push(Chunk { kind, base: b, mine: m, theirs: t }),
            }
        }
    } else {
        chunks.push(Chunk { kind: kind(b.clone(), m.clone(), t.clone()), base: b, mine: m, theirs: t });
    }
}

/// For each paragraph of `base`, where it is in `other` if the diff kept it.
fn matches(base: &[String], other: &[String]) -> Vec<Option<usize>> {
    let mut found = vec![None; base.len()];
    for op in capture_diff_slices(Algorithm::Myers, base, other) {
        if let DiffOp::Equal { old_index, new_index, len } = op {
            for n in 0..len {
                found[old_index + n] = Some(new_index + n);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "One.\n\nTwo.\n\nThree.\n\nFour.\n";

    #[test]
    fn paragraphs_are_split_on_blank_lines() {
        assert_eq!(paragraphs("\nOne\nstill one.\n\n\n  \nTwo.\r\n"), ["One\nstill one.", "Two."]);
        assert_eq!(join(&["One.", "Two."]), "One.\n\nTwo.\n");
        assert_eq!(join::<&str>(&[]), "");
    }

    #[test]
    fn changes_on_one_side_each_are_taken() {
        let merged = merge(BASE, "One!\n\nTwo.\n\nThree.\n\nFour.\n", "One.\n\nTwo.\n\nThree.\n\nFour!\n\nFive.\n");
        assert_eq!(merged.text, "One!\n\nTwo.\n\nThree.\n\nFour!\n\nFive.\n");
        assert_eq!(merged.clashes, 0);
        assert_eq!(merged.base, merged.text);
        assert_eq!(merged.theirs, merged.text);
    }

    #[test]
    fn neighbouring_paragraphs_changed_on_different_sides_dont_clash() {
        let merged = merge(BASE, "One.\n\nTwo!\n\nThree.\n\nFour.\n", "One.\n\nTwo.\n\nThree?\n\nFour.\n");
        assert_eq!(merged.text, "One.\n\nTwo!\n\nThree?\n\nFour.\n");
        assert_eq!(merged.clashes, 0);
    }

    #[test]
    fn the_same_change_on_both_sides_isnt_a_clash() {
        let both = "One.\n\nTwo, again.\n\nThree.\n\nFour.\n";
        assert_eq!(merge(BASE, both, both).clashes, 0);
    }

    #[test]
    fn a_clash_keeps_mine_and_remembers_theirs() {
        let mine = "One.\n\nTwo, mine.\n\nThree.\n\nFour!\n";
        let theirs = "One?\n\nTwo, theirs.\n\nThree.\n\nFour.\n";
        let merged = merge(BASE, mine, theirs);
        assert_eq!(merged.clashes, 1);
        assert_eq!(merged.text, "One?\n\nTwo, mine.\n\nThree.\n\nFour!\n");
        assert_eq!(merged.base, "One?\n\nTwo.\n\nThree.\n\nFour!\n");
        assert_eq!(merged.theirs, "One?\n\nTwo, theirs.\n\nThree.\n\nFour!\n");

        let found = clashes(&merged.base, &merged.text, &merged.theirs);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].mine, 1..2);
        assert_eq!(found[0].mine_text, ["Two, mine."]);
        assert_eq!(found[0].theirs_text, ["Two, theirs."]);
    }

    #[test]
    fn a_clash_is_found_again_after_editing_elsewhere_or_in_it() {
        let merged = merge(BASE, "One.\n\nTwo, mine.\n\nThree.\n\nFour.\n", "One.\n\nTwo, theirs.\n\nThree.\n\nFour.\n");
        let edited = "Zero.\n\nOne.\n\nTwo, mine and more.\n\nThree.\n\nFour.\n";
        let found = clashes(&merged.base, edited, &merged.theirs);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].mine, 2..3);
        assert_eq!(found[0].mine_text, ["Two, mine and more."]);
    }

    #[test]
    fn removing_on_one_side_and_changing_on_the_other_clashes() {
        let merged = merge(BASE, "One.\n\nThree.\n\nFour.\n", "One.\n\nTwo, theirs.\n\nThree.\n\nFour.\n");
        assert_eq!(merged.clashes, 1);
        let found = clashes(&merged.base, &merged.text, &merged.theirs);
        assert_eq!(found[0].mine, 1..1);
        assert!(found[0].mine_text.is_empty());
        assert_eq!(found[0].theirs_text, ["Two, theirs."]);
    }

    #[test]
    fn neighbouring_clashes_are_one() {
        let merged = merge(BASE, "One.\n\nTwo, mine.\n\nThree, mine.\n\nFour.\n", "One.\n\nTwo, theirs.\n\nThree, theirs.\n\nFour.\n");
        assert_eq!(merged.clashes, 1);
    }

    #[test]
    fn settling_each_way_leaves_no_clash_and_later_edits_are_mine() {
        let merged = merge(BASE, "One.\n\nTwo, mine.\n\nThree.\n\nFour.\n", "One.\n\nTwo, theirs.\n\nThree.\n\nFour.\n");
        let clash = &clashes(&merged.base, &merged.text, &merged.theirs)[0];
        for (choice, expected) in [
            (Choice::Theirs, "One.\n\nTwo, theirs.\n\nThree.\n\nFour.\n"),
            (Choice::Mine, "One.\n\nTwo, mine.\n\nThree.\n\nFour.\n"),
            (Choice::Both, "One.\n\nTwo, mine.\n\nTwo, theirs.\n\nThree.\n\nFour.\n"),
        ] {
            let settled = settle(&merged.base, &merged.text, &merged.theirs, clash, choice);
            assert_eq!(settled.mine, expected, "{choice:?}");
            assert!(clashes(&settled.base, &settled.mine, &settled.theirs).is_empty(), "{choice:?}");
            let later = settled.mine.replace("Two,", "Two again,");
            assert!(clashes(&settled.base, &later, &settled.theirs).is_empty(), "{choice:?}");
        }
    }

    #[test]
    fn settling_one_clash_leaves_the_others() {
        let merged = merge(BASE, "One, mine.\n\nTwo.\n\nThree, mine.\n\nFour.\n", "One, theirs.\n\nTwo.\n\nThree, theirs.\n\nFour.\n");
        assert_eq!(merged.clashes, 2);
        let found = clashes(&merged.base, &merged.text, &merged.theirs);
        let settled = settle(&merged.base, &merged.text, &merged.theirs, &found[1], Choice::Theirs);
        let left = clashes(&settled.base, &settled.mine, &settled.theirs);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].mine_text, ["One, mine."]);
    }
}
