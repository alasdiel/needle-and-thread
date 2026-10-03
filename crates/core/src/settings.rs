//! Vault-wide settings in `.needle/vault.toml`. Missing fields fall back to the defaults.

use serde::Deserialize;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct TypographySettings {
    pub double_quotes: bool,
    pub single_quotes: bool,
    pub em_dash: bool,
    pub ellipsis: bool,
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
        toml::from_str(toml).map_err(|e| format!(".needle/vault.toml isn't valid: {}", e.message()))
    }
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
}
