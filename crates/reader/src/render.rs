//! A shared scene as HTML. Each run of text is a `<span data-o>` saying where it starts in the
//! scene's prose (`needle_core::anchor`), so a selection can be turned back into an anchor, and
//! words with comments on them are wrapped in `<mark class="cm" data-c="<comment id>">`. HTML
//! written in the text is shown as text, never run; the editor's underline is kept.

use std::ops::Range;

use needle_core::anchor::{Walk, events, is_underline};
use pulldown_cmark::{CowStr, Event, html};

/// Words a comment is on: characters of the scene's prose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    pub range: Range<usize>,
    pub id: u64,
}

pub fn html(markdown: &str, marks: &[Mark]) -> String {
    let mut walk = Walk::default();
    let events = events(markdown).into_iter().map(|event| {
        let at = walk.step(&event);
        match (event, at) {
            (Event::Text(text), Some(at)) => Event::InlineHtml(CowStr::from(spans(&text, at, marks))),
            (Event::Code(text), Some(at)) => Event::InlineHtml(CowStr::from(format!("<code>{}</code>", spans(&text, at, marks)))),
            (Event::Html(text), Some(at)) => Event::Html(CowStr::from(spans(&text, at, marks))),
            (Event::InlineHtml(text), Some(at)) => Event::InlineHtml(CowStr::from(spans(&text, at, marks))),
            (Event::InlineHtml(text), None) if is_underline(&text) => Event::InlineHtml(text),
            (event, _) => event,
        }
    });
    let mut out = String::new();
    html::push_html(&mut out, events);
    out
}

/// `text`, starting at `at` in the prose, as spans, with the marked stretches wrapped.
fn spans(text: &str, at: usize, marks: &[Mark]) -> String {
    let chars: Vec<char> = text.chars().collect();
    let end = at + chars.len();
    let mut cuts = vec![at, end];
    for mark in marks {
        cuts.extend([mark.range.start, mark.range.end].into_iter().filter(|&c| c > at && c < end));
    }
    cuts.sort_unstable();
    cuts.dedup();
    let mut out = String::new();
    for pair in cuts.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        let piece: String = chars[from - at..to - at].iter().collect();
        let span = format!(r#"<span data-o="{from}">{}</span>"#, escape(&piece));
        let on: Vec<u64> = marks.iter().filter(|m| m.range.start <= from && from < m.range.end).map(|m| m.id).collect();
        match on.first() {
            Some(id) => out.push_str(&format!(r#"<mark class="cm{}" data-c="{id}">{span}</mark>"#, if on.len() > 1 { " many" } else { "" })),
            None => out.push_str(&span),
        }
    }
    out
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use needle_core::anchor::prose;

    #[test]
    fn text_says_where_it_starts_in_the_prose() {
        assert_eq!(
            html("A *soft* line\nand [[Teodor Brask|Old Teodor]].", &[]),
            "<p><span data-o=\"0\">A </span><em><span data-o=\"2\">soft</span></em><span data-o=\"6\"> line</span>\n<span data-o=\"12\">and Old Teodor.</span></p>\n"
        );
        let p = prose("A *soft* line\nand [[Teodor Brask|Old Teodor]].");
        assert_eq!(p.chars().skip(16).take(10).collect::<String>(), "Old Teodor");
    }

    #[test]
    fn commented_words_are_marked_even_across_emphasis() {
        let out = html("lit *before* the bells", &[Mark { range: 4..16, id: 7 }]);
        assert_eq!(
            out,
            "<p><span data-o=\"0\">lit </span><em><mark class=\"cm\" data-c=\"7\"><span data-o=\"4\">before</span></mark></em><mark class=\"cm\" data-c=\"7\"><span data-o=\"10\"> the b</span></mark><span data-o=\"16\">ells</span></p>\n"
        );
    }

    #[test]
    fn written_html_is_shown_not_run_but_underline_stays() {
        let out = html("A <script>alert(1)</script> and <u>this</u>.", &[]);
        assert!(!out.contains("<script>"), "{out}");
        assert!(out.contains("&lt;script&gt;"), "{out}");
        assert!(out.contains("<u><span data-o="), "{out}");
    }
}
