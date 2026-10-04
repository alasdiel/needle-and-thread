//! `[[wikilinks]]` in Markdown, read the way the editor reads them (`wikilinks` in
//! editor/src/markdown.ts), and the form names are compared in.

use std::ops::Range;

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
    fn name_keys_ignore_case_spacing_and_apostrophe_style() {
        assert_eq!(name_key("  Old   Teodor "), "old teodor");
        assert_eq!(name_key("Mara’s Ship"), name_key("mara's ship"));
        assert_eq!(name_key("ÉLODIE"), "élodie");
    }
}
