//! Word-level differences between two versions of a text, for the history panel.

use similar::{ChangeTag, TextDiff};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Same,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub kind: Kind,
    pub text: String,
}

/// Runs of unchanged, added and removed text that together cover both versions. Neighbouring
/// pieces of the same kind are merged, and so are words rewritten together: "[-Old-]{+Brand+}
/// [-second.-]{+new.+}" becomes "[-Old second.-]{+Brand new.+}".
pub fn words(old: &str, new: &str) -> Vec<Segment> {
    let diff = TextDiff::from_words(old, new);
    let mut segments: Vec<Segment> = Vec::new();
    for change in diff.iter_all_changes() {
        let kind = match change.tag() {
            ChangeTag::Equal => Kind::Same,
            ChangeTag::Insert => Kind::Added,
            ChangeTag::Delete => Kind::Removed,
        };
        push(&mut segments, kind, change.value());
    }
    join_rewrites(segments)
}

fn push(segments: &mut Vec<Segment>, kind: Kind, text: &str) {
    match segments.last_mut() {
        Some(last) if last.kind == kind => last.text.push_str(text),
        _ => segments.push(Segment { kind, text: text.to_owned() }),
    }
}

/// Changes with only spaces between them, that both remove and add words, become one removal
/// and one addition.
fn join_rewrites(segments: Vec<Segment>) -> Vec<Segment> {
    let is_space = |s: &Segment| s.kind == Kind::Same && s.text.trim().is_empty();
    let mut joined = Vec::new();
    let mut i = 0;
    while i < segments.len() {
        if segments[i].kind == Kind::Same {
            push(&mut joined, Kind::Same, &segments[i].text);
            i += 1;
            continue;
        }
        // The run of changes from here, across spaces between them.
        let is_change = |j: usize| segments.get(j).is_some_and(|s| s.kind != Kind::Same);
        let mut end = i + 1;
        loop {
            if is_change(end) {
                end += 1;
            } else if is_change(end + 1) && is_space(&segments[end]) {
                end += 2;
            } else {
                break;
            }
        }
        let run = &segments[i..end];
        if run.iter().any(|s| s.kind == Kind::Removed) && run.iter().any(|s| s.kind == Kind::Added) {
            let side = |kind: Kind| run.iter().filter(|s| s.kind != kind).map(|s| s.text.as_str()).collect::<String>();
            push(&mut joined, Kind::Removed, &side(Kind::Added));
            push(&mut joined, Kind::Added, &side(Kind::Removed));
        } else {
            for s in run {
                push(&mut joined, s.kind, &s.text);
            }
        }
        i = end;
    }
    joined
}

/// Whether removed segment `index` can be put back: it has some text, not just spacing.
pub fn can_put_back(segments: &[Segment], index: usize) -> bool {
    segments.get(index).is_some_and(|s| s.kind == Kind::Removed && !s.text.trim().is_empty())
}

/// The new text with removed segment `index` put back where it was. If something replaced it,
/// both stay: the old passage goes in beside the new one, a space apart (or a blank line, for
/// whole paragraphs), and nothing is lost. None if `index` isn't a removal that can go back.
pub fn put_back(segments: &[Segment], index: usize) -> Option<String> {
    if !can_put_back(segments, index) {
        return None;
    }
    let removed = &segments[index].text;
    let neighbour = |i: Option<usize>| i.and_then(|i| segments.get(i));
    let (before, after) = (neighbour(index.checked_sub(1)), neighbour(Some(index + 1)));
    // Whole paragraphs: a paragraph break (or the text's edge) on both sides.
    let starts_paragraph = before.is_none_or(|s| s.text.ends_with('\n'));
    let ends_paragraph = segments[index + 1..]
        .iter()
        .find(|s| s.kind != Kind::Added)
        .is_none_or(|s| s.text.starts_with('\n'));
    let gap = if starts_paragraph && ends_paragraph { "\n\n" } else { " " };
    let needs_gap = |left: &str, right: &str| !left.ends_with(char::is_whitespace) && !right.starts_with(char::is_whitespace);

    let mut text = String::new();
    for (i, segment) in segments.iter().enumerate() {
        match segment.kind {
            Kind::Same | Kind::Added => text.push_str(&segment.text),
            Kind::Removed if i != index => {}
            Kind::Removed => {
                if let Some(added) = before.filter(|s| s.kind == Kind::Added)
                    && needs_gap(&added.text, removed)
                {
                    text.push_str(gap);
                }
                text.push_str(removed);
                if let Some(added) = after.filter(|s| s.kind == Kind::Added)
                    && needs_gap(removed, &added.text)
                {
                    text.push_str(gap);
                }
            }
        }
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(segments: &[Segment]) -> String {
        segments
            .iter()
            .map(|s| match s.kind {
                Kind::Same => s.text.clone(),
                Kind::Added => format!("{{+{}+}}", s.text),
                Kind::Removed => format!("[-{}-]", s.text),
            })
            .collect()
    }

    #[test]
    fn marks_added_and_removed_words() {
        let segments = words("The market opened at dusk.", "The night market opened at dawn.");
        assert_eq!(render(&segments), "The {+night +}market opened at [-dusk.-]{+dawn.+}");
    }

    #[test]
    fn words_rewritten_together_are_one_change() {
        let segments = words("First. Old second. Third.", "First. Brand new. Third.");
        assert_eq!(render(&segments), "First. [-Old second.-]{+Brand new.+} Third.");
        // Only removals (or only additions) across a space stay apart, as the space is unchanged.
        let segments = words("a b c d", "a c");
        assert!(render(&segments).starts_with("a "), "{}", render(&segments));
    }

    #[test]
    fn joined_changes_still_cover_both_versions() {
        let (old, new) = ("She walked the stalls twice, as if counting.", "Mara walked the length of it twice, counting stalls.");
        let segments = words(old, new);
        let keep = |k: Kind| segments.iter().filter(|s| s.kind != k).map(|s| s.text.as_str()).collect::<String>();
        assert_eq!(keep(Kind::Added), old);
        assert_eq!(keep(Kind::Removed), new);
    }

    #[test]
    fn identical_texts_are_one_unchanged_run() {
        assert_eq!(
            words("Same text.", "Same text."),
            vec![Segment {
                kind: Kind::Same,
                text: "Same text.".into()
            }]
        );
    }

    #[test]
    fn segments_cover_both_versions() {
        let (old, new) = ("one two three", "one 2 three four");
        let segments = words(old, new);
        let keep = |k: Kind| segments.iter().filter(|s| s.kind != k).map(|s| s.text.as_str()).collect::<String>();
        assert_eq!(keep(Kind::Added), old);
        assert_eq!(keep(Kind::Removed), new);
    }

    /// The text with the first removal (whose text contains `which`) put back.
    fn put_back_one(old: &str, new: &str, which: &str) -> String {
        let segments = words(old, new);
        let index = segments
            .iter()
            .position(|s| s.kind == Kind::Removed && s.text.contains(which))
            .expect("no such removal");
        put_back(&segments, index).unwrap()
    }

    #[test]
    fn puts_a_removed_sentence_back_where_it_was() {
        let old = "The smell came in. A fiddler was tuning, badly. She kept the compass.";
        let new = "The smell came in. She kept the compass.";
        assert_eq!(put_back_one(old, new, "fiddler"), old);
    }

    #[test]
    fn puts_back_one_of_several_removals() {
        let old = "One. Two. Three. Four.";
        let new = "One. Three.";
        assert_eq!(put_back_one(old, new, "Two"), "One. Two. Three.");
        assert_eq!(put_back_one(old, new, "Four"), "One. Three. Four.");
    }

    #[test]
    fn puts_a_removed_paragraph_back() {
        let old = "First.\n\nSecond.\n\nThird.\n";
        let new = "First.\n\nThird.\n";
        assert_eq!(put_back_one(old, new, "Second"), old);
    }

    #[test]
    fn a_replaced_passage_goes_back_beside_its_replacement() {
        assert_eq!(put_back_one("The market opened at dusk.", "The market opened at dawn.", "dusk"), "The market opened at dusk. dawn.");
    }

    #[test]
    fn a_replaced_paragraph_goes_back_as_its_own_paragraph() {
        let old = "First.\n\nOld second.\n\nThird.";
        let new = "First.\n\nBrand new.\n\nThird.";
        assert_eq!(put_back_one(old, new, "Old"), "First.\n\nOld second.\n\nBrand new.\n\nThird.");
    }

    #[test]
    fn spacing_and_added_text_cant_be_put_back() {
        let segments = words("one  two", "one two three");
        for (i, s) in segments.iter().enumerate() {
            if s.kind != Kind::Removed || s.text.trim().is_empty() {
                assert_eq!(put_back(&segments, i), None, "{s:?}");
            }
        }
    }
}
