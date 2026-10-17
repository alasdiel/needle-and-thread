//! Where a comment on shared text sits, and how it keeps its place as the text changes
//! (DESIGN §10, "Keeping comments attached").
//!
//! A comment is anchored by the words it's on (`quote`), which of that phrase's occurrences it
//! is (`n`), and a little text either side. All of it is measured in a scene's *prose*: the text
//! a reader sees, with every run of spaces and line breaks read as one space, so the desktop's
//! editor, the reader page and this module agree on it without sharing a renderer.

use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, TagEnd};
use serde::{Deserialize, Serialize};
use similar::{Algorithm, DiffTag, capture_diff_slices};

use crate::links::rewrite_links;

/// How much text either side of the quote a comment keeps, in characters.
const CONTEXT: usize = 32;

/// A scene's Markdown as the reader page parses it: wikilinks are their labels (notes are never
/// shared), and HTML written in the text stays text.
pub fn events(markdown: &str) -> Vec<Event<'static>> {
    let markdown = rewrite_links(markdown, |link| Some(escape(link.label.unwrap_or(link.target))));
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    Parser::new_ext(&markdown, options).map(Event::into_static).collect()
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '*' | '_' | '[' | ']' | '<' | '>' | '`' | '~') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Whether `html` is the editor's underline, which readers see as underlining, not as text.
pub fn is_underline(html: &str) -> bool {
    matches!(html, "<u>" | "</u>")
}

/// Builds a scene's prose from its events, one at a time, so a renderer can tell where in the
/// prose each piece of text it writes starts.
#[derive(Debug, Default)]
pub struct Walk {
    text: String,
    chars: usize,
}

impl Walk {
    /// Takes the next event. For text a reader sees, returns where it starts, in characters.
    pub fn step(&mut self, event: &Event) -> Option<usize> {
        match event {
            Event::Text(text) | Event::Code(text) => Some(self.push(text)),
            Event::Html(html) | Event::InlineHtml(html) if !is_underline(html) => Some(self.push(html)),
            Event::SoftBreak | Event::HardBreak => {
                self.push(" ");
                None
            }
            Event::End(end) if is_block(end) => {
                self.push("\n");
                None
            }
            Event::Rule => {
                self.push("\n");
                None
            }
            _ => None,
        }
    }

    fn push(&mut self, text: &str) -> usize {
        let at = self.chars;
        self.text.push_str(text);
        self.chars += text.chars().count();
        at
    }

    pub fn finish(self) -> String {
        self.text
    }
}

fn is_block(end: &TagEnd) -> bool {
    matches!(end, TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::BlockQuote(_) | TagEnd::CodeBlock | TagEnd::Item | TagEnd::List(_) | TagEnd::HtmlBlock)
}

/// A scene's prose, before spaces are folded: what `Walk` builds from the whole scene.
pub fn prose(markdown: &str) -> String {
    let mut walk = Walk::default();
    for event in events(markdown) {
        walk.step(&event);
    }
    walk.finish()
}

/// Text with every run of whitespace as one space, and where each of its characters came from
/// in the text (plus one past the end).
struct Folded {
    chars: Vec<char>,
    from: Vec<usize>,
}

fn fold(text: &str) -> Folded {
    let mut chars = Vec::with_capacity(text.len());
    let mut from = Vec::with_capacity(text.len() + 1);
    let mut count = 0;
    for (i, c) in text.chars().enumerate() {
        count = i + 1;
        if c.is_whitespace() {
            if chars.last() == Some(&' ') {
                continue;
            }
            chars.push(' ');
        } else {
            chars.push(c);
        }
        from.push(i);
    }
    from.push(count);
    Folded { chars, from }
}

/// `text` with every run of whitespace as one space, trimmed.
pub fn folded(text: &str) -> String {
    fold(text).chars.into_iter().collect::<String>().trim().to_owned()
}

/// Where `needle` starts in `hay`, every time.
fn occurrences(hay: &[char], needle: &[char]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return Vec::new();
    }
    (0..=hay.len() - needle.len()).filter(|&i| hay[i..i + needle.len()] == *needle).collect()
}

/// Where a comment is: the words it's on, as prose, folded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    pub quote: String,
    /// How many times the quote comes earlier in the scene.
    #[serde(default)]
    pub n: usize,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub before: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub after: String,
}

impl Anchor {
    /// The anchor for the characters `range` of `prose`, or None if they're only spaces.
    pub fn at(prose: &str, range: Range<usize>) -> Option<Self> {
        let folded = fold(prose);
        // The folded characters that came from inside `range`, without spaces at the ends.
        let mut start = folded.from.partition_point(|&i| i < range.start);
        let mut end = folded.from[..folded.chars.len()].partition_point(|&i| i < range.end);
        while start < end && folded.chars[start] == ' ' {
            start += 1;
        }
        while end > start && folded.chars[end - 1] == ' ' {
            end -= 1;
        }
        if start >= end {
            return None;
        }
        let quote = &folded.chars[start..end];
        let n = occurrences(&folded.chars, quote).iter().filter(|&&i| i < start).count();
        Some(Self {
            quote: quote.iter().collect(),
            n,
            before: folded.chars[start.saturating_sub(CONTEXT)..start].iter().collect(),
            after: folded.chars[end..(end + CONTEXT).min(folded.chars.len())].iter().collect(),
        })
    }

    /// Where the anchor is in `prose`, taken exactly: the quote's `n`th occurrence.
    pub fn find(&self, prose: &str) -> Option<Range<usize>> {
        let folded = fold(prose);
        let quote: Vec<char> = self.quote.chars().collect();
        let start = *occurrences(&folded.chars, &quote).get(self.n)?;
        Some(folded.raw(start..start + quote.len()))
    }

    /// Where the quote is in `prose`, wherever it went: of all its occurrences, the one whose
    /// surroundings are most like the anchor's, the earliest if that's a tie.
    pub fn search(&self, prose: &str) -> Option<Range<usize>> {
        let folded = fold(prose);
        let quote: Vec<char> = self.quote.chars().collect();
        let before: Vec<char> = self.before.chars().collect();
        let after: Vec<char> = self.after.chars().collect();
        let score = |start: usize| {
            let ahead = folded.chars[..start].iter().rev().zip(before.iter().rev()).take_while(|(a, b)| a == b).count();
            let behind = folded.chars[start + quote.len()..].iter().zip(&after).take_while(|(a, b)| a == b).count();
            ahead + behind
        };
        let best = occurrences(&folded.chars, &quote).into_iter().rev().max_by_key(|&start| score(start))?;
        Some(folded.raw(best..best + quote.len()))
    }
}

impl Folded {
    /// The characters of the original text that `range` of the folded text came from.
    fn raw(&self, range: Range<usize>) -> Range<usize> {
        self.from[range.start]..self.from[range.end - 1] + 1
    }
}

/// Follows the characters `range` of `old` into `new`: from the first of them still there to the
/// last, taking in whatever was written between. None if none of them are left.
pub fn follow(old: &str, new: &str, range: Range<usize>) -> Option<Range<usize>> {
    let old: Vec<char> = old.chars().collect();
    let new: Vec<char> = new.chars().collect();
    if range.is_empty() || range.end > old.len() {
        return None;
    }
    let ops = capture_diff_slices(Algorithm::Myers, &old, &new);
    // Where an old character is now, if it's still there.
    let kept = |i: usize| {
        ops.iter().find_map(|op| {
            let (tag, from, to) = op.as_tag_tuple();
            (tag == DiffTag::Equal && from.contains(&i)).then(|| to.start + (i - from.start))
        })
    };
    let start = range.clone().find_map(kept)?;
    let end = range.rev().find_map(kept)? + 1;
    (start < end).then_some(start..end)
}

/// Where a comment made on `then` is in `now`, or None if its words are gone. Its words are
/// followed through the changes from `then`, if that's known; failing that, searched for.
pub fn reanchor(anchor: &Anchor, then: Option<&str>, now: &str) -> Option<Anchor> {
    let followed = then.and_then(|then| follow(then, now, anchor.find(then)?));
    let range = followed.or_else(|| anchor.search(now))?;
    Anchor::at(now, range)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCENE: &str = "The market opened at dusk, as it always had,\nand nobody asked why the lanterns were lit before the bells.\n\nMara walked the length of it twice, counting stalls, and once more because her feet would not stop. She was [[Mara Venn|counting]] again.\n";

    fn quote(prose: &str, words: &str) -> Range<usize> {
        let start = prose.find(words).unwrap();
        let start = prose[..start].chars().count();
        start..start + words.chars().count()
    }

    fn text(prose: &str, range: Range<usize>) -> String {
        prose.chars().skip(range.start).take(range.len()).collect()
    }

    #[test]
    fn prose_is_what_a_reader_sees() {
        assert_eq!(
            prose("A *soft* line\nthat wraps, and [[Teodor Brask|Old Teodor]].\n\n- one\n- two\n\n---\n\nAfter `code` and <u>this</u>."),
            "A soft line that wraps, and Old Teodor.\none\ntwo\n\n\nAfter code and this.\n"
        );
        assert_eq!(folded(" a\n\n b  c "), "a b c");
    }

    #[test]
    fn prose_folds_the_same_as_the_editor_does() {
        // editor/test/comments.test.ts folds the same scene from the editor's document.
        let scene = "The market opened at dusk, as it always had,\nand nobody asked why the lanterns were lit before the bells.\n\nMara walked the length of it twice, *counting* stalls, and once more. She was [[Mara Venn|counting]] again.\n";
        assert_eq!(
            folded(&prose(scene)),
            "The market opened at dusk, as it always had, and nobody asked why the lanterns were lit before the bells. Mara walked the length of it twice, counting stalls, and once more. She was counting again."
        );
    }

    #[test]
    fn an_anchor_finds_its_words_again() {
        let prose = prose(SCENE);
        let range = quote(&prose, "lit before the bells.\nMara walked");
        let anchor = Anchor::at(&prose, range.clone()).unwrap();
        assert_eq!(anchor.quote, "lit before the bells. Mara walked", "spaces folded");
        assert_eq!(anchor.n, 0);
        assert!(anchor.before.ends_with("the lanterns were "), "{anchor:?}");
        assert!(anchor.after.starts_with(" the length"), "{anchor:?}");
        assert_eq!(anchor.find(&prose), Some(range));
        assert_eq!(Anchor::at(&prose, 0..0), None);
    }

    #[test]
    fn the_same_words_twice_are_told_apart() {
        let prose = prose(SCENE);
        let second = prose.rfind("counting").unwrap();
        let second = prose[..second].chars().count();
        let anchor = Anchor::at(&prose, second..second + 8).unwrap();
        assert_eq!((anchor.quote.as_str(), anchor.n), ("counting", 1));
        assert_eq!(anchor.find(&prose), Some(second..second + 8));
        assert_eq!(anchor.search(&prose), Some(second..second + 8), "its surroundings pick it out");
    }

    #[test]
    fn a_comment_follows_its_words_through_edits() {
        let then = prose(SCENE);
        let anchor = Anchor::at(&then, quote(&then, "lit before the bells")).unwrap();
        let now = prose(&SCENE.replace("The market opened", "Every year the market opened").replace("lit before", "lit long before"));
        let moved = reanchor(&anchor, Some(&then), &now).unwrap();
        assert_eq!(moved.quote, "lit long before the bells", "takes in what was written inside it");
        assert_eq!(text(&now, moved.find(&now).unwrap()), "lit long before the bells");
    }

    #[test]
    fn without_the_old_text_the_quote_is_searched_for() {
        let then = prose(SCENE);
        let anchor = Anchor::at(&then, quote(&then, "counting stalls")).unwrap();
        let now = prose(&format!("A new first paragraph about counting stalls elsewhere.\n\n{SCENE}"));
        let found = reanchor(&anchor, None, &now).unwrap();
        assert_eq!((found.quote.as_str(), found.n), ("counting stalls", 1), "the one in its old surroundings");
    }

    #[test]
    fn words_cut_from_the_text_leave_the_comment_without_a_place() {
        let then = prose(SCENE);
        let anchor = Anchor::at(&then, quote(&then, "her feet would not stop")).unwrap();
        let now = prose(&SCENE.replace(" and once more because her feet would not stop.", ""));
        assert_eq!(reanchor(&anchor, Some(&then), &now), None);
        assert_eq!(reanchor(&anchor, None, &now), None);
    }

    #[test]
    fn follow_keeps_what_survives_at_the_edges() {
        assert_eq!(follow("abc def ghi", "abc XYZ ghi", 4..7), None, "all of it replaced");
        assert_eq!(follow("abc def ghi", "abc de ghi", 4..7), Some(4..6));
        assert_eq!(follow("abc def ghi", "new abc def ghi", 4..7), Some(8..11));
    }
}
