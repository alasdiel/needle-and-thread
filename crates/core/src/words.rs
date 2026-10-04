/// Counts the words a reader sees in a scene's Markdown, matching the editor's count: a
/// wikilink counts its label, a link counts its text but not its URL, and `<u>` tags don't count.
pub fn count_markdown_words(markdown: &str) -> usize {
    count_words(&visible_text(markdown))
}

pub(crate) fn visible_text(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut rest = markdown;
    while let Some(ch) = rest.chars().next() {
        if let Some(after) = rest.strip_prefix("[[")
            && let Some(end) = after.find("]]")
        {
            let inner = &after[..end];
            out.push_str(inner.rsplit_once('|').map_or(inner, |(_, label)| label));
            rest = &after[end + 2..];
        } else if let Some(after) = rest.strip_prefix("](")
            && let Some(end) = after.find(')')
        {
            out.push(' ');
            rest = &after[end + 1..];
        } else if let Some(after) = rest.strip_prefix("<u>").or_else(|| rest.strip_prefix("</u>")) {
            rest = after;
        } else {
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    out
}

/// A scene or note as plain text, for searching and quoting: what a reader sees, without
/// Markdown's markers. Paragraphs stay on lines of their own; scene breaks become blank lines.
pub fn plain_text(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    for line in visible_text(markdown).lines() {
        let trimmed = line.trim();
        if matches!(trimmed, "---" | "***" | "* * *") {
            out.push('\n');
            continue;
        }
        let mut text = trimmed.trim_start_matches('>').trim_start();
        text = text.trim_start_matches('#').trim_start();
        if let Some(rest) = text.strip_prefix("- ").or_else(|| text.strip_prefix("+ ")) {
            text = rest;
        } else if let Some((number, rest)) = text.split_once(". ")
            && !number.is_empty()
            && number.chars().all(|c| c.is_ascii_digit())
        {
            text = rest;
        }
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                // Emphasis markers, and the backslash of an escaped character.
                '*' => {}
                '\\' => {
                    if let Some(next) = chars.next() {
                        out.push(next);
                    }
                }
                c => out.push(c),
            }
        }
        out.push('\n');
    }
    out
}

/// Counts words the way the editor does: runs of letters or digits, joined across an inner
/// apostrophe or hyphen ("don't", "well-known").
pub fn count_words(text: &str) -> usize {
    let mut count = 0;
    let mut in_word = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_alphanumeric() {
            if !in_word {
                count += 1;
                in_word = true;
            }
        } else if !(in_word
            && matches!(c, '\'' | '’' | '-')
            && chars.peek().is_some_and(|next| next.is_alphanumeric()))
        {
            in_word = false;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_like_the_editor() {
        assert_eq!(count_words(""), 0);
        assert_eq!(count_words("Mara’s ship—the *Gull*—sank."), 5);
        assert_eq!(count_words("don't stop, well-known 1998"), 4);
        assert_eq!(count_words("trailing - dash and 'quotes'"), 4);
    }

    #[test]
    fn plain_text_drops_markdown_markers() {
        let md = "## The *harbour*\n\n> [[Mara Venn|Mara]] said \\*no\\*.\n\n---\n\n- one\n1. two <u>three</u>\n";
        assert_eq!(plain_text(md), "The harbour\n\nMara said *no*.\n\n\n\none\ntwo three\n");
    }

    #[test]
    fn markdown_counts_only_what_a_reader_sees() {
        assert_eq!(count_markdown_words("[[Mara Venn|Mara]] met [[Old Teodor]]."), 4);
        assert_eq!(count_markdown_words("See [the map](https://example.com/old-map \"Old map\")."), 3);
        assert_eq!(count_markdown_words("Some <u>underlined</u> words."), 3);
        assert_eq!(count_markdown_words("*Emphasis* and **bold**\n\n---\n\n- a list item"), 6);
    }
}
