//! A scene's or note's Markdown as HTML for reading. Wikilinks become buttons carrying the name
//! they link to, which the page opens as a sheet; HTML written in the text is shown as text,
//! never run.

use needle_core::links::rewrite_links;
use pulldown_cmark::{CowStr, Event, LinkType, Options, Parser, Tag, TagEnd, html};

const SCHEME: &str = "needle-link:";

pub fn html(markdown: &str) -> String {
    // Each wikilink becomes an ordinary link to its place in `targets`, which Markdown parses
    // reliably, then goes back to being a wikilink below.
    let mut targets = Vec::new();
    let markdown = rewrite_links(markdown, |link| {
        targets.push(link.target.to_owned());
        let label = link.label.unwrap_or(link.target);
        Some(format!("[{}]({SCHEME}{})", label.replace(['[', ']'], ""), targets.len() - 1))
    });
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_SMART_PUNCTUATION);
    let mut in_wikilink = false;
    let events = Parser::new_ext(&markdown, options).map(|event| match event {
        Event::Start(Tag::Link { link_type: LinkType::Inline, ref dest_url, .. }) if dest_url.starts_with(SCHEME) => {
            let target = dest_url[SCHEME.len()..].parse::<usize>().ok().and_then(|i| targets.get(i)).cloned().unwrap_or_default();
            in_wikilink = true;
            Event::Html(CowStr::from(format!(r#"<button type="button" class="wl" data-target="{}">"#, escape(&target))))
        }
        Event::End(TagEnd::Link) if in_wikilink => {
            in_wikilink = false;
            Event::Html(CowStr::from("</button>"))
        }
        Event::Html(text) | Event::InlineHtml(text) => Event::Text(text),
        event => event,
    });
    let mut out = String::new();
    html::push_html(&mut out, events);
    out
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::html;

    #[test]
    fn wikilinks_become_buttons_with_their_target() {
        let out = html("[[Mara Venn|Mara]] walked to [[Old Teodor]].");
        assert_eq!(
            out,
            "<p><button type=\"button\" class=\"wl\" data-target=\"Mara Venn\">Mara</button> walked to <button type=\"button\" class=\"wl\" data-target=\"Old Teodor\">Old Teodor</button>.</p>\n"
        );
    }

    #[test]
    fn written_html_is_shown_not_run() {
        let out = html("A <script>alert(1)</script> here.");
        assert!(!out.contains("<script>"), "{out}");
        assert!(out.contains("&lt;script&gt;"), "{out}");
    }

    #[test]
    fn ordinary_markdown_still_renders() {
        assert_eq!(html("*Soft* and **firm**."), "<p><em>Soft</em> and <strong>firm</strong>.</p>\n");
    }
}
