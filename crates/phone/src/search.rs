//! Search on the phone. The desktop's index is SQLite, which a browser can't run, so this looks
//! through the texts already in memory: everything fetched is a few hundred files at most.

/// Something to look through: its title and the text a reader sees.
pub struct Doc<T> {
    pub item: T,
    pub title: String,
    pub text: String,
}

pub struct Hit<T> {
    pub item: T,
    pub title: String,
    /// The text around the first match, and the match itself, to mark.
    pub before: String,
    pub matched: String,
    pub after: String,
}

/// Docs holding every word of `query` (case aside), titles that match first.
pub fn search<T: Clone>(docs: &[Doc<T>], query: &str) -> Vec<Hit<T>> {
    let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if terms.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<(bool, Hit<T>)> = Vec::new();
    for doc in docs {
        let title = doc.title.to_lowercase();
        let text = doc.text.to_lowercase();
        if !terms.iter().all(|t| title.contains(t.as_str()) || text.contains(t.as_str())) {
            continue;
        }
        let in_title = terms.iter().all(|t| title.contains(t.as_str()));
        let (before, matched, after) = snippet(&doc.text, &text, &terms[0]);
        hits.push((in_title, Hit { item: doc.item.clone(), title: doc.title.clone(), before, matched, after }));
    }
    hits.sort_by_key(|(in_title, _)| !in_title);
    hits.into_iter().map(|(_, hit)| hit).collect()
}

/// About forty characters either side of the first place `term` is in `text`; `lower` is
/// `text` lowercased. Empty when it isn't there (a title-only match).
fn snippet(text: &str, lower: &str, term: &str) -> (String, String, String) {
    // Lowercasing can change lengths outside ASCII, so only trust positions that line up.
    let Some(at) = lower.find(term).filter(|&at| lower.len() == text.len() && text.is_char_boundary(at) && text.is_char_boundary(at + term.len())) else {
        return (String::new(), String::new(), String::new());
    };
    // Runs of spaces and line breaks become one space, keeping one at either end.
    let flat = |s: &str| {
        let inner = s.split_whitespace().collect::<Vec<_>>().join(" ");
        let lead = if s.starts_with(char::is_whitespace) && !inner.is_empty() { " " } else { "" };
        let trail = if s.ends_with(char::is_whitespace) && !inner.is_empty() { " " } else { "" };
        format!("{lead}{inner}{trail}")
    };
    let start = floor(text, at.saturating_sub(40));
    let end = ceil(text, (at + term.len() + 40).min(text.len()));
    let before = flat(&text[start..at]);
    let after = flat(&text[at + term.len()..end]);
    (
        if start > 0 { format!("…{}", before.trim_start()) } else { before.trim_start().to_owned() },
        text[at..at + term.len()].to_owned(),
        if end < text.len() { format!("{}…", after.trim_end()) } else { after.trim_end().to_owned() },
    )
}

fn floor(s: &str, mut i: usize) -> usize {
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil(s: &str, mut i: usize) -> usize {
    while !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(title: &str, text: &str) -> Doc<String> {
        Doc { item: title.to_owned(), title: title.to_owned(), text: text.to_owned() }
    }

    #[test]
    fn every_word_has_to_be_there_and_titles_come_first() {
        let docs = [doc("The harbor", "Tar and salt."), doc("Market", "The harbor smelled of salt."), doc("Quay", "Salt only.")];
        // A word can be in the title and another in the text.
        let hits = search(&docs, "harbor salt");
        assert_eq!(hits.iter().map(|h| h.item.as_str()).collect::<Vec<_>>(), ["The harbor", "Market"]);
        let hits = search(&docs, "harbor tar");
        assert_eq!(hits.iter().map(|h| h.item.as_str()).collect::<Vec<_>>(), ["The harbor"]);
        let hits = search(&docs, "harbor");
        assert_eq!(hits.iter().map(|h| h.item.as_str()).collect::<Vec<_>>(), ["The harbor", "Market"]);
    }

    #[test]
    fn the_snippet_keeps_the_match_as_written() {
        let hits = search(&[doc("Market", "She kept the Compass wrapped in her sleeve.")], "compass");
        assert_eq!((hits[0].before.as_str(), hits[0].matched.as_str(), hits[0].after.as_str()), ("She kept the ", "Compass", " wrapped in her sleeve."));
    }

    #[test]
    fn nothing_is_found_for_nothing() {
        assert!(search(&[doc("A", "b")], "  ").is_empty());
    }
}
