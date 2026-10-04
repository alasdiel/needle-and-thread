//! `[[wikilinks]]` in Markdown, read the way the editor reads them (`wikilinks` in
//! editor/src/markdown.ts), and the form names are compared in.

use std::ops::Range;

use crate::words::visible_text;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link<'a> {
    /// The name linked to, trimmed: `Mara Venn` in `[[Mara Venn|Mara]]`.
    pub target: &'a str,
    /// The text shown instead of the name, if any: `Mara`.
    pub label: Option<&'a str>,
    /// Byte range of the whole `[[…]]` in the Markdown.
    pub range: Range<usize>,
}

/// Every wikilink in `markdown`, in order. A backslash-escaped `\[` never starts one, and a
/// link can't span lines or contain brackets, just as in the editor.
pub fn links(markdown: &str) -> Vec<Link<'_>> {
    let bytes = markdown.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            // Markdown escapes any ASCII punctuation; an escaped `[` is plain text.
            b'\\' if bytes.get(i + 1).is_some_and(u8::is_ascii_punctuation) => i += 2,
            b'[' if bytes.get(i + 1) == Some(&b'[') => match link_at(markdown, i) {
                Some(link) => {
                    i = link.range.end;
                    found.push(link);
                }
                None => i += 1,
            },
            _ => i += 1,
        }
    }
    found
}

fn link_at(markdown: &str, start: usize) -> Option<Link<'_>> {
    let inner_start = start + 2;
    let inner_end = inner_start + markdown[inner_start..].find("]]")?;
    let inner = &markdown[inner_start..inner_end];
    if inner.contains(['[', ']', '\n']) {
        return None;
    }
    let (target, label) = match inner.split_once('|') {
        Some((target, label)) => (target.trim(), Some(label.trim()).filter(|l| !l.is_empty())),
        None => (inner.trim(), None),
    };
    if target.is_empty() {
        return None;
    }
    Some(Link {
        target,
        label,
        range: start..inner_end + 2,
    })
}

/// Replaces links: `replace` gets each one and returns its new Markdown, or None to keep it.
pub fn rewrite_links(markdown: &str, mut replace: impl FnMut(&Link) -> Option<String>) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut done = 0;
    for link in links(markdown) {
        if let Some(new) = replace(&link) {
            out.push_str(&markdown[done..link.range.start]);
            out.push_str(&new);
            done = link.range.end;
        }
    }
    out.push_str(&markdown[done..]);
    out
}

/// A link as Markdown: `[[target]]`, or `[[target|label]]` when the label differs.
pub fn format_link(target: &str, label: Option<&str>) -> String {
    match label {
        Some(label) if label != target => format!("[[{target}|{label}]]"),
        _ => format!("[[{target}]]"),
    }
}

/// Where `name` appears as a whole word outside links. Case counts, since names are proper
/// nouns ("Will" shouldn't find "will"), but apostrophe style doesn't, and so a possessive
/// ("Mara’s") is found too.
pub fn mentions(markdown: &str, name: &str) -> Vec<Range<usize>> {
    let name = name.trim();
    if name.is_empty() {
        return Vec::new();
    }
    let linked: Vec<Range<usize>> = links(markdown).into_iter().map(|l| l.range).collect();
    let mut variants = vec![name.to_owned()];
    for (from, to) in [('\'', "’"), ('’', "'")] {
        if name.contains(from) {
            variants.push(name.replace(from, to));
        }
    }
    let is_word_char = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
    let mut found: Vec<Range<usize>> = variants
        .iter()
        .flat_map(|variant| markdown.match_indices(variant.as_str()).map(|(i, m)| i..i + m.len()))
        .filter(|r| !is_word_char(markdown[..r.start].chars().next_back()) && !is_word_char(markdown[r.end..].chars().next()))
        .filter(|r| !linked.iter().any(|l| l.start < r.end && r.start < l.end))
        .collect();
    found.sort_by_key(|r| r.start);
    found.dedup();
    found
}

/// The text around `range` (a mention) as a reader sees it, from the same paragraph: up to
/// `radius` characters each side, cut at a space, with "…" where it was cut.
pub fn snippet(markdown: &str, range: Range<usize>, radius: usize) -> (String, String, String) {
    let start = markdown[..range.start].rfind("\n\n").map_or(0, |i| i + 2);
    let end = markdown[range.end..].find("\n\n").map_or(markdown.len(), |i| range.end + i);
    let plain = |md: &str| {
        let text = visible_text(md).replace(['*', '\\'], "");
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    let before = plain(&markdown[start..range.start]);
    let before = before.trim_start_matches(['#', '>', '-', ' ']);
    let after = plain(&markdown[range.end..end]);

    let chars: Vec<char> = before.chars().collect();
    let before = if chars.len() > radius {
        let cut: String = chars[chars.len() - radius..].iter().collect();
        format!("…{}", cut.split_once(' ').map_or(cut.as_str(), |(_, rest)| rest))
    } else {
        before.to_owned()
    };
    let after = if after.chars().count() > radius {
        let cut: String = after.chars().take(radius).collect();
        format!("{}…", cut.rsplit_once(' ').map_or(cut.as_str(), |(rest, _)| rest))
    } else {
        after
    };
    // Keep the space that separated the name from its neighbours.
    let space = |s: &str, at_end: bool| match at_end {
        true if markdown[..range.start].ends_with(char::is_whitespace) && !s.is_empty() => format!("{s} "),
        false if markdown[range.end..].starts_with(char::is_whitespace) && !s.is_empty() => format!(" {s}"),
        _ => s.to_owned(),
    };
    (space(&before, true), markdown[range.clone()].to_owned(), space(&after, false))
}

/// The form names are compared in, so `[[old teodor]]` finds "Old Teodor": lowercase, curly
/// apostrophes straightened, and any run of whitespace as one space.
pub fn name_key(name: &str) -> String {
    let mut key = String::with_capacity(name.len());
    for word in name.split_whitespace() {
        if !key.is_empty() {
            key.push(' ');
        }
        for c in word.chars().flat_map(char::to_lowercase) {
            key.push(if c == '’' { '\'' } else { c });
        }
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(markdown: &str) -> Vec<(&str, Option<&str>)> {
        links(markdown).into_iter().map(|l| (l.target, l.label)).collect()
    }

    #[test]
    fn finds_links_and_labels() {
        let md = "[[Mara Venn|Mara]] met [[ Old Teodor ]] at the [[Night Market|]].";
        assert_eq!(
            targets(md),
            [("Mara Venn", Some("Mara")), ("Old Teodor", None), ("Night Market", None)]
        );
        let first = &links(md)[0];
        assert_eq!(&md[first.range.clone()], "[[Mara Venn|Mara]]");
    }

    #[test]
    fn skips_what_the_editor_does_not_read_as_a_link() {
        assert!(targets("[[]] [[ | label]] [[a]b]] [[split\nline]] [[unclosed").is_empty());
        assert!(targets(r"\[[Not a link]]").is_empty());
        assert_eq!(targets(r"\\[[Linked]]"), [("Linked", None)]);
        assert_eq!(targets("[[[Teodor]]"), [("Teodor", None)]);
        assert_eq!(targets("a [link](x) and [[Mara]]"), [("Mara", None)]);
    }

    #[test]
    fn handles_text_outside_ascii() {
        assert_eq!(targets("Café — [[Zoë’s ship|the ship]] …"), [("Zoë’s ship", Some("the ship"))]);
    }

    #[test]
    fn rewrites_only_the_links_asked_for() {
        let md = "[[Old Teodor]] and [[Mara]], then [[old teodor|him]].";
        let out = rewrite_links(md, |l| {
            (name_key(l.target) == "old teodor").then(|| format_link("Teodor Brask", Some(l.label.unwrap_or(l.target))))
        });
        assert_eq!(out, "[[Teodor Brask|Old Teodor]] and [[Mara]], then [[Teodor Brask|him]].");
        assert_eq!(format_link("Mara", Some("Mara")), "[[Mara]]");
    }

    #[test]
    fn mentions_are_whole_words_outside_links() {
        let md = "Mara’s ship. [[Mara]] and Marabou, but MARA and Mara.";
        let found: Vec<&str> = mentions(md, "Mara").into_iter().map(|r| &md[r]).collect();
        assert_eq!(found, ["Mara", "Mara"]);
        assert_eq!(mentions(md, "Mara")[0].start, 0);
        let md = "Teodor's stall, Teodor’s stall";
        assert_eq!(mentions(md, "Teodor's stall").len(), 2);
        assert!(mentions("anything", "  ").is_empty());
    }

    #[test]
    fn snippets_show_the_sentence_around_a_mention() {
        let md = "First paragraph.\n\nThe lanterns *guttered*, and Mara counted the stalls a third time, slowly.\n\nNext.";
        let range = mentions(md, "Mara")[0].clone();
        let (before, name, after) = snippet(md, range.clone(), 200);
        assert_eq!(
            (before.as_str(), name.as_str(), after.as_str()),
            ("The lanterns guttered, and ", "Mara", " counted the stalls a third time, slowly.")
        );
        let (before, _, after) = snippet(md, range, 14);
        assert_eq!((before.as_str(), after.as_str()), ("…guttered, and ", " counted the…"));
    }

    #[test]
    fn name_keys_ignore_case_spacing_and_apostrophe_style() {
        assert_eq!(name_key("  Old   Teodor "), "old teodor");
        assert_eq!(name_key("Mara’s Ship"), name_key("mara's ship"));
        assert_eq!(name_key("ÉLODIE"), "élodie");
    }
}
