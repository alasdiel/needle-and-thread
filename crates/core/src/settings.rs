//! Vault-wide settings in `.needle/vault.toml`. Missing fields fall back to the defaults. The
//! file is the user's too, so the app's edits go through toml_edit and keep the rest as written.

use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Item, Value};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct VaultSettings {
    pub version: u32,
    /// Scene statuses, in order.
    pub statuses: Vec<String>,
    pub snapshots: SnapshotSettings,
    pub typography: TypographySettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SnapshotSettings {
    pub idle_minutes: u64,
    pub max_minutes: u64,
}

/// Automatic typography changes, each switchable on its own. Mirrors `Typography` in
/// editor/src/inputrules.ts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TypographySettings {
    pub double_quotes: bool,
    pub single_quotes: bool,
    pub em_dash: bool,
    pub ellipsis: bool,
}

/// One of the typography changes, named (in serde too) by its key under `[typography]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TypographyRule {
    DoubleQuotes,
    SingleQuotes,
    EmDash,
    Ellipsis,
}

impl TypographyRule {
    pub fn key(self) -> &'static str {
        match self {
            Self::DoubleQuotes => "double_quotes",
            Self::SingleQuotes => "single_quotes",
            Self::EmDash => "em_dash",
            Self::Ellipsis => "ellipsis",
        }
    }
}

impl TypographySettings {
    pub fn is_on(&self, rule: TypographyRule) -> bool {
        match rule {
            TypographyRule::DoubleQuotes => self.double_quotes,
            TypographyRule::SingleQuotes => self.single_quotes,
            TypographyRule::EmDash => self.em_dash,
            TypographyRule::Ellipsis => self.ellipsis,
        }
    }

    pub fn set(&mut self, rule: TypographyRule, on: bool) {
        let switch = match rule {
            TypographyRule::DoubleQuotes => &mut self.double_quotes,
            TypographyRule::SingleQuotes => &mut self.single_quotes,
            TypographyRule::EmDash => &mut self.em_dash,
            TypographyRule::Ellipsis => &mut self.ellipsis,
        };
        *switch = on;
    }
}

impl Default for VaultSettings {
    fn default() -> Self {
        Self {
            version: 1,
            statuses: ["idea", "draft", "revised", "done"].map(str::to_owned).to_vec(),
            snapshots: SnapshotSettings::default(),
            typography: TypographySettings::default(),
        }
    }
}

impl Default for SnapshotSettings {
    fn default() -> Self {
        Self {
            idle_minutes: 2,
            max_minutes: 10,
        }
    }
}

impl Default for TypographySettings {
    fn default() -> Self {
        Self {
            double_quotes: true,
            single_quotes: true,
            em_dash: true,
            ellipsis: true,
        }
    }
}

impl VaultSettings {
    pub fn parse(toml: &str) -> Result<Self, String> {
        toml::from_str(toml).map_err(|e| invalid(e.message()))
    }
}

/// Switches one typography change in the text of `.needle/vault.toml`, adding the key (and the
/// `[typography]` table) if it isn't there. Everything else, comments included, stays as written.
pub fn set_typography(toml: &str, rule: TypographyRule, on: bool) -> Result<String, String> {
    let mut doc: DocumentMut = toml.parse().map_err(|e: toml_edit::TomlError| invalid(e.message()))?;
    let table = doc
        .entry("typography")
        .or_insert_with(toml_edit::table)
        .as_table_like_mut()
        .ok_or_else(|| invalid("typography isn't a table"))?;
    match table.get_mut(rule.key()) {
        // Keeps the key's position and the comments around it.
        Some(Item::Value(existing)) => {
            let mut value = Value::from(on);
            *value.decor_mut() = existing.decor().clone();
            *existing = value;
        }
        _ => {
            table.insert(rule.key(), toml_edit::value(on));
        }
    }
    Ok(doc.to_string())
}

fn invalid(message: &str) -> String {
    format!(".needle/vault.toml isn't valid: {message}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_use_defaults() {
        let s = VaultSettings::parse("statuses = [\"outline\", \"draft\"]\n[typography]\nem_dash = false\n").unwrap();
        assert_eq!(s.statuses, ["outline", "draft"]);
        assert!(!s.typography.em_dash);
        assert!(s.typography.ellipsis);
        assert_eq!(s.snapshots, SnapshotSettings::default());
    }

    #[test]
    fn empty_file_is_all_defaults() {
        assert_eq!(VaultSettings::parse("").unwrap(), VaultSettings::default());
    }

    #[test]
    fn switching_typography_keeps_the_rest_as_written() {
        let before = r#"# My vault.
version = 1

[typography]
# Curly quotes.
double_quotes = true   # always
em_dash = true

[snapshots]
idle_minutes = 5
"#;
        let after = set_typography(before, TypographyRule::DoubleQuotes, false).unwrap();
        assert_eq!(after, before.replace("double_quotes = true", "double_quotes = false"));
        assert!(!VaultSettings::parse(&after).unwrap().typography.double_quotes);
    }

    #[test]
    fn switching_a_missing_typography_key_adds_it() {
        let after = set_typography("[typography]\nem_dash = true\n", TypographyRule::Ellipsis, false).unwrap();
        assert_eq!(after, "[typography]\nem_dash = true\nellipsis = false\n");
        let after = set_typography("version = 1\n", TypographyRule::EmDash, false).unwrap();
        assert!(after.contains("[typography]"));
        assert!(!VaultSettings::parse(&after).unwrap().typography.em_dash);
    }

    #[test]
    fn switching_typography_in_a_broken_file_fails() {
        assert!(set_typography("version = [", TypographyRule::EmDash, false).is_err());
        assert!(set_typography("typography = 3\n", TypographyRule::EmDash, false).is_err());
    }

    #[test]
    fn rules_name_their_keys() {
        use TypographyRule::*;
        let mut settings = TypographySettings::default();
        for rule in [DoubleQuotes, SingleQuotes, EmDash, Ellipsis] {
            assert_eq!(toml::Value::try_from(rule).unwrap().as_str(), Some(rule.key()));
            settings.set(rule, false);
            assert!(!settings.is_on(rule));
        }
        let all_off = "[typography]\ndouble_quotes = false\nsingle_quotes = false\nem_dash = false\nellipsis = false\n";
        assert_eq!(VaultSettings::parse(all_off).unwrap().typography, settings);
    }
}
