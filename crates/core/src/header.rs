//! The TOML header at the top of scenes and notes. Edits go through toml_edit, so whatever the
//! app doesn't change (formatting, comments, fields it doesn't know about) stays as written.

use std::fmt;

use toml_edit::{Array, DocumentMut, Item, Value};

#[derive(Debug, Clone, Default)]
pub struct Header {
    doc: DocumentMut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderError(String);

impl fmt::Display for HeaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the header isn't valid TOML: {}", self.0)
    }
}

impl std::error::Error for HeaderError {}

impl Header {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn parse(toml: &str) -> Result<Self, HeaderError> {
        toml.parse()
            .map(|doc| Self { doc })
            .map_err(|e: toml_edit::TomlError| HeaderError(e.message().to_owned()))
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        self.doc.get(key)?.as_str()
    }

    pub fn set_str(&mut self, key: &str, value: &str) {
        self.set_value(key, Value::from(value));
    }

    pub fn bool(&self, key: &str) -> Option<bool> {
        self.doc.get(key)?.as_bool()
    }

    pub fn set_bool(&mut self, key: &str, value: bool) {
        self.set_value(key, Value::from(value));
    }

    /// A list of strings. Anything that isn't a string is skipped; a single string counts as a
    /// one-item list.
    pub fn list(&self, key: &str) -> Vec<String> {
        match self.doc.get(key) {
            Some(Item::Value(Value::Array(items))) => {
                items.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()
            }
            Some(item) => item.as_str().map(|s| vec![s.to_owned()]).unwrap_or_default(),
            None => Vec::new(),
        }
    }

    pub fn set_list<S: AsRef<str>>(&mut self, key: &str, items: &[S]) {
        let array: Array = items.iter().map(AsRef::as_ref).collect();
        self.set_value(key, Value::Array(array));
    }

    /// A list of small tables, like a relationship's `changes = [{ at = "…", label = "…" }]`,
    /// with each table's string fields. Written inline or as `[[changes]]`, it reads the same;
    /// anything that isn't a table, and any field that isn't a string, is skipped.
    pub fn tables(&self, key: &str) -> Vec<Vec<(String, String)>> {
        let strings = |pairs: Vec<(&str, &Value)>| -> Vec<(String, String)> {
            pairs.into_iter().filter_map(|(k, v)| Some((k.to_owned(), v.as_str()?.to_owned()))).collect()
        };
        match self.doc.get(key) {
            Some(Item::Value(Value::Array(items))) => items
                .iter()
                .filter_map(Value::as_inline_table)
                .map(|t| strings(t.iter().collect()))
                .collect(),
            Some(Item::ArrayOfTables(tables)) => tables
                .iter()
                .map(|t| strings(t.iter().filter_map(|(k, item)| Some((k, item.as_value()?))).collect()))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// A small table's string fields, like `when = { from = "The harbor", offset = "+6h" }`,
    /// written inline or as a `[when]` section. Fields that aren't strings are skipped.
    pub fn table(&self, key: &str) -> Option<Vec<(String, String)>> {
        let pairs: Vec<(&str, &Value)> = match self.doc.get(key)? {
            Item::Value(Value::InlineTable(t)) => t.iter().collect(),
            Item::Table(t) => t.iter().filter_map(|(k, item)| Some((k, item.as_value()?))).collect(),
            _ => return None,
        };
        Some(pairs.into_iter().filter_map(|(k, v)| Some((k.to_owned(), v.as_str()?.to_owned()))).collect())
    }

    /// Writes a small inline table of strings: `when = { after = "The harbor" }`.
    pub fn set_table(&mut self, key: &str, fields: &[(&str, &str)]) {
        let mut table = toml_edit::InlineTable::new();
        for (k, v) in fields {
            table.insert(*k, Value::from(*v));
        }
        table.fmt();
        if matches!(self.doc.get(key), Some(Item::Table(_))) {
            self.doc.remove(key);
        }
        self.set_value(key, Value::InlineTable(table));
    }

    /// A string, or a TOML date (`when = 1998-03-14T19:00:00`) as it's written.
    pub fn text(&self, key: &str) -> Option<String> {
        match self.doc.get(key)?.as_value()? {
            Value::String(s) => Some(s.value().clone()),
            Value::Datetime(d) => Some(d.value().to_string()),
            _ => None,
        }
    }

    pub fn remove(&mut self, key: &str) {
        self.doc.remove(key);
    }

    pub fn contains(&self, key: &str) -> bool {
        self.doc.contains_key(key)
    }

    pub fn id(&self) -> Option<&str> {
        self.str("id")
    }

    pub fn title(&self) -> Option<&str> {
        self.str("title")
    }

    /// Replaces a value but keeps the key's position and its surrounding comments.
    fn set_value(&mut self, key: &str, mut value: Value) {
        if let Some(Item::Value(existing)) = self.doc.get_mut(key) {
            *value.decor_mut() = existing.decor().clone();
            *existing = value;
        } else {
            self.doc.insert(key, Item::Value(value));
        }
    }
}

impl fmt::Display for Header {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCENE: &str = r#"id = "sc_7f3k9q"
title = "The night market"   # working title
status = "draft"

# Who's in it
cast = ["Mara Venn", "Old Teodor"]
when = { from = "The harbor", offset = "+6h" }
mood = "uneasy"
"#;

    #[test]
    fn reads_fields() {
        let h = Header::parse(SCENE).unwrap();
        assert_eq!(h.id(), Some("sc_7f3k9q"));
        assert_eq!(h.title(), Some("The night market"));
        assert_eq!(h.list("cast"), ["Mara Venn", "Old Teodor"]);
        assert_eq!(h.list("status"), ["draft"]);
        assert!(h.list("places").is_empty());
    }

    #[test]
    fn unchanged_header_round_trips_exactly() {
        assert_eq!(Header::parse(SCENE).unwrap().to_string(), SCENE);
    }

    #[test]
    fn edits_keep_formatting_comments_and_unknown_fields() {
        let mut h = Header::parse(SCENE).unwrap();
        h.set_str("status", "revised");
        h.set_str("title", "The night market, again");
        h.set_list("cast", &["Mara Venn"]);
        assert_eq!(
            h.to_string(),
            r#"id = "sc_7f3k9q"
title = "The night market, again"   # working title
status = "revised"

# Who's in it
cast = ["Mara Venn"]
when = { from = "The harbor", offset = "+6h" }
mood = "uneasy"
"#
        );
    }

    #[test]
    fn new_fields_go_at_the_end() {
        let mut h = Header::parse("id = \"sc_1\"\n").unwrap();
        h.set_str("status", "idea");
        h.set_list("places", &["Night Market"]);
        assert_eq!(h.to_string(), "id = \"sc_1\"\nstatus = \"idea\"\nplaces = [\"Night Market\"]\n");
    }

    #[test]
    fn reads_a_list_of_tables_either_way_its_written() {
        let inline = Header::parse(r#"changes = [{ at = "The harbor", label = "distrusts" }, "stray", { at = 3 }]"#).unwrap();
        assert_eq!(
            inline.tables("changes"),
            [vec![("at".to_owned(), "The harbor".to_owned()), ("label".to_owned(), "distrusts".to_owned())], vec![]]
        );
        let long = Header::parse("[[changes]]\nat = \"The harbor\"\nlabel = \"distrusts\"\n").unwrap();
        assert_eq!(long.tables("changes"), inline.tables("changes")[..1]);
        assert!(inline.tables("nothing").is_empty());
    }

    #[test]
    fn reads_a_table_and_a_date() {
        let h = Header::parse(SCENE).unwrap();
        assert_eq!(h.table("when").unwrap(), [("from".to_owned(), "The harbor".to_owned()), ("offset".to_owned(), "+6h".to_owned())]);
        assert_eq!(h.table("title"), None);
        let long = Header::parse("title = \"x\"\n[when]\nafter = \"The harbor\"\n").unwrap();
        assert_eq!(long.table("when").unwrap(), [("after".to_owned(), "The harbor".to_owned())]);
        let mut edited = Header::parse(SCENE).unwrap();
        edited.set_table("when", &[("after", "The harbor"), ("before", "The Drowning")]);
        assert!(edited.to_string().contains("\nwhen = { after = \"The harbor\", before = \"The Drowning\" }\nmood"));
        let mut long = long;
        long.set_table("when", &[("from", "Dawn")]);
        assert_eq!(long.to_string(), "title = \"x\"\nwhen = { from = \"Dawn\" }\n");
        let dated = Header::parse("when = 1998-03-14T19:00:00\nalso = \"3 Thaw\"").unwrap();
        assert_eq!(dated.text("when").as_deref(), Some("1998-03-14T19:00:00"));
        assert_eq!(dated.text("also").as_deref(), Some("3 Thaw"));
    }

    #[test]
    fn invalid_toml_is_an_error() {
        assert!(Header::parse("title = unquoted words").is_err());
    }
}
