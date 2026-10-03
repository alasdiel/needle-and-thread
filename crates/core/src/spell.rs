//! Spellchecking against Hunspell dictionaries. spellbook is pure Rust, so this runs inside
//! the wasm frontend and checks words synchronously as the editor asks.

use spellbook::Dictionary;

pub use spellbook::ParseDictionaryError;

pub struct Speller {
    dictionary: Dictionary,
}

impl Speller {
    pub fn new(aff: &str, dic: &str) -> Result<Self, ParseDictionaryError> {
        Ok(Self {
            dictionary: Dictionary::new(aff, dic)?,
        })
    }

    /// Also accepts curly apostrophes and the possessive of any accepted word ("Teodor’s").
    pub fn check(&self, word: &str) -> bool {
        let word = word.replace('’', "'");
        self.dictionary.check(&word)
            || word
                .strip_suffix("'s")
                .or_else(|| word.strip_suffix("'S"))
                .is_some_and(|base| !base.is_empty() && self.dictionary.check(base))
    }

    /// Suggestions in the dictionary's order, using the same apostrophe style as `word`.
    pub fn suggest(&self, word: &str) -> Vec<String> {
        let mut suggestions = Vec::new();
        self.dictionary.suggest(&word.replace('’', "'"), &mut suggestions);
        if word.contains('’') {
            for s in &mut suggestions {
                *s = s.replace('\'', "’");
            }
        }
        suggestions
    }

    /// Accepts `word` from now on. Persisting it is the caller's job.
    pub fn add(&mut self, word: &str) -> Result<(), InvalidWord> {
        if !is_addable(word) {
            return Err(InvalidWord);
        }
        // `/` would start Hunspell affix flags; is_addable rules it out.
        self.dictionary
            .add(&word.replace('’', "'"))
            .map_err(|_| InvalidWord)
    }
}

/// A single word: no whitespace, no `/`, not empty.
pub fn is_addable(word: &str) -> bool {
    !word.is_empty() && !word.contains('/') && !word.chars().any(char::is_whitespace)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidWord;

impl std::fmt::Display for InvalidWord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("not a single word")
    }
}

impl std::error::Error for InvalidWord {}

#[cfg(test)]
mod tests {
    use super::*;

    const AFF: &str = "SET UTF-8\nTRY esianrtolcdugmphbyfvkwzESIANRTOLCDUGMPHBYFVKWZ'\n";
    const DIC: &str = "4\nhello\nworld\ndon't\nharbor\n";

    fn speller() -> Speller {
        Speller::new(AFF, DIC).unwrap()
    }

    #[test]
    fn checks_dictionary_words() {
        let s = speller();
        assert!(s.check("hello"));
        assert!(!s.check("helo"));
    }

    #[test]
    fn accepts_curly_apostrophes() {
        let s = speller();
        assert!(s.check("don't"));
        assert!(s.check("don’t"));
    }

    #[test]
    fn accepts_possessives_of_known_words() {
        let s = speller();
        assert!(s.check("harbor's"));
        assert!(s.check("harbor’s"));
        assert!(!s.check("harbr’s"));
        assert!(!s.check("'s"));
    }

    #[test]
    fn suggests_corrections_in_the_same_apostrophe_style() {
        let s = speller();
        assert!(s.suggest("helo").contains(&"hello".to_owned()));
        assert!(s.suggest("dont’").iter().all(|w| !w.contains('\'')));
    }

    #[test]
    fn added_words_are_accepted() {
        let mut s = speller();
        assert!(!s.check("Teodor"));
        s.add("Teodor").unwrap();
        assert!(s.check("Teodor"));
        assert!(s.check("Teodor’s"));
    }

    /// Needs a system en_US Hunspell dictionary: `cargo test -p needle-core -- --ignored`.
    #[test]
    #[ignore]
    fn system_dictionary_smoke() {
        let read = |ext: &str| std::fs::read_to_string(format!("/usr/share/hunspell/en_US.{ext}")).unwrap();
        let started = std::time::Instant::now();
        let s = Speller::new(&read("aff"), &read("dic")).unwrap();
        eprintln!("loaded in {:?}", started.elapsed());
        for word in ["harbor", "don’t", "Mara’s", "lanterns"] {
            assert!(s.check(word), "{word} should be accepted");
        }
        for word in ["definately", "recieve", "wierd", "seperate", "Teodor"] {
            assert!(!s.check(word), "{word} should be flagged");
            eprintln!("{word} → {:?}", s.suggest(word));
        }
    }

    #[test]
    fn rejects_words_that_are_not_single_words() {
        let mut s = speller();
        assert_eq!(s.add("two words"), Err(InvalidWord));
        assert_eq!(s.add("flags/G"), Err(InvalidWord));
        assert_eq!(s.add(""), Err(InvalidWord));
    }
}
