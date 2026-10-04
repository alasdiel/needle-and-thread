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
    fn markdown_counts_only_what_a_reader_sees() {
        assert_eq!(count_markdown_words("[[Mara Venn|Mara]] met [[Old Teodor]]."), 4);
        assert_eq!(count_markdown_words("See [the map](https://example.com/old-map \"Old map\")."), 3);
        assert_eq!(count_markdown_words("Some <u>underlined</u> words."), 3);
        assert_eq!(count_markdown_words("*Emphasis* and **bold**\n\n---\n\n- a list item"), 6);
    }
}
