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
/// pieces of the same kind are merged.
pub fn words(old: &str, new: &str) -> Vec<Segment> {
    let diff = TextDiff::from_words(old, new);
    let mut segments: Vec<Segment> = Vec::new();
    for change in diff.iter_all_changes() {
        let kind = match change.tag() {
            ChangeTag::Equal => Kind::Same,
            ChangeTag::Insert => Kind::Added,
            ChangeTag::Delete => Kind::Removed,
        };
        match segments.last_mut() {
            Some(last) if last.kind == kind => last.text.push_str(change.value()),
            _ => segments.push(Segment {
                kind,
                text: change.value().to_owned(),
            }),
        }
    }
    segments
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
}
