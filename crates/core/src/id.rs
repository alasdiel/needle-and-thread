//! Stable ids for scenes, notes and chapters, and file names derived from titles.

const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// `prefix` plus 10 characters (50 bits) taken from `random`, e.g. `sc_7f3k9qa2mx`. The caller
/// supplies the randomness so this stays free of I/O.
pub fn make_id(prefix: &str, random: u64) -> String {
    let mut id = String::with_capacity(prefix.len() + 11);
    id.push_str(prefix);
    id.push('_');
    for i in (0..10).rev() {
        id.push(ALPHABET[((random >> (i * 5)) & 31) as usize] as char);
    }
    id
}

/// A file name for `title`: lowercase letters and digits joined by hyphens.
pub fn slugify(title: &str) -> String {
    let mut slug = String::with_capacity(title.len());
    for c in title.chars().flat_map(char::to_lowercase) {
        if c == '\'' || c == '’' {
            continue;
        } else if c.is_alphanumeric() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() { "untitled".to_owned() } else { slug.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_have_prefix_and_ten_characters() {
        let id = make_id("sc", 0x0123_4567_89ab_cdef);
        assert!(id.starts_with("sc_"));
        assert_eq!(id.len(), 13);
        assert_ne!(make_id("sc", 1), make_id("sc", 2));
    }

    #[test]
    fn slugs_from_titles() {
        assert_eq!(slugify("The Night Market"), "the-night-market");
        assert_eq!(slugify("  Café — at dawn!  "), "café-at-dawn");
        assert_eq!(slugify("Mara’s ship"), "maras-ship");
        assert_eq!(slugify("???"), "untitled");
    }
}
