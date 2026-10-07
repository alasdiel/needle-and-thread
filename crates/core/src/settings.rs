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
    pub backup: BackupSettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SnapshotSettings {
    pub idle_minutes: u64,
    pub max_minutes: u64,
}

/// Pushing the vault's history to a repository on GitHub (or another git host), over SSH.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BackupSettings {
    /// The repository's SSH address, e.g. `git@github.com:you/novel.git`; empty for none.
    pub remote: String,
    /// Push after each snapshot.
    pub after_snapshot: bool,
}

impl BackupSettings {
    /// Whether to push after each snapshot.
    pub fn is_on(&self) -> bool {
        self.after_snapshot && !self.remote.trim().is_empty()
    }
}

impl Default for BackupSettings {
    fn default() -> Self {
        Self {
            remote: String::new(),
            after_snapshot: true,
        }
    }
}

/// Checks a backup address: backups push over SSH, so it must be an SSH address, like
/// `git@github.com:you/novel.git` or `ssh://git@github.com/you/novel.git`. Empty is fine (no
/// backup).
pub fn check_remote(remote: &str) -> Result<(), String> {
    let remote = remote.trim();
    if remote.is_empty() || remote.starts_with("ssh://") {
        return Ok(());
    }
    if remote.starts_with("https://") || remote.starts_with("http://") {
        return Err("use the repository's SSH address, like git@github.com:you/novel.git".into());
    }
    // scp-style: user@host:path
    match remote.split_once(':') {
        Some((host, path)) if host.contains('@') && !host.contains('/') && !path.is_empty() => Ok(()),
        _ => Err("that isn't an SSH address; it looks like git@github.com:you/novel.git".into()),
    }
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
            backup: BackupSettings::default(),
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
    set_key(&mut doc, "typography", rule.key(), Value::from(on))?;
    Ok(doc.to_string())
}

/// Sets the backup in the text of `.needle/vault.toml`, like `set_typography`.
pub fn set_backup(toml: &str, backup: &BackupSettings) -> Result<String, String> {
    let mut doc: DocumentMut = toml.parse().map_err(|e: toml_edit::TomlError| invalid(e.message()))?;
    set_key(&mut doc, "backup", "remote", Value::from(backup.remote.trim()))?;
    set_key(&mut doc, "backup", "after_snapshot", Value::from(backup.after_snapshot))?;
    Ok(doc.to_string())
}

/// Sets `key` in `[table]`, adding either if needed and keeping the comments around the key.
fn set_key(doc: &mut DocumentMut, table: &str, key: &str, mut value: Value) -> Result<(), String> {
    let table = doc
        .entry(table)
        .or_insert_with(toml_edit::table)
        .as_table_like_mut()
        .ok_or_else(|| invalid(&format!("{table} isn't a table")))?;
    match table.get_mut(key) {
        // Keeps the key's position and the comments around it.
        Some(Item::Value(existing)) => {
            *value.decor_mut() = existing.decor().clone();
            *existing = value;
        }
        _ => {
            table.insert(key, toml_edit::value(value));
        }
    }
    Ok(())
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
    fn setting_the_backup_keeps_the_rest_as_written() {
        let before = "# My vault.\nversion = 1\n\n[typography]\nem_dash = true\n";
        let backup = BackupSettings {
            remote: " git@github.com:me/novel.git ".into(),
            after_snapshot: true,
        };
        let after = set_backup(before, &backup).unwrap();
        assert!(after.starts_with(before), "{after}");
        let parsed = VaultSettings::parse(&after).unwrap().backup;
        assert_eq!(parsed.remote, "git@github.com:me/novel.git");
        assert!(parsed.is_on());

        let off = set_backup(&after, &BackupSettings { after_snapshot: false, ..parsed }).unwrap();
        assert_eq!(off, after.replace("after_snapshot = true", "after_snapshot = false"));
        assert!(!VaultSettings::parse(&off).unwrap().backup.is_on());
    }

    #[test]
    fn no_backup_by_default() {
        let backup = VaultSettings::parse("").unwrap().backup;
        assert_eq!(backup.remote, "");
        assert!(!backup.is_on());
    }

    #[test]
    fn backup_addresses_must_be_ssh() {
        for ok in ["", "git@github.com:me/novel.git", "ssh://git@github.com/me/novel.git", "me@host.example:vaults/novel"] {
            assert_eq!(check_remote(ok), Ok(()), "{ok}");
        }
        for bad in ["https://github.com/me/novel.git", "github.com/me/novel", "git@github.com:", "/home/me/backup.git"] {
            assert!(check_remote(bad).is_err(), "{bad}");
        }
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
