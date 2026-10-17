//! Opening the vault on the phone: the repository's files, fetched (or taken from the cache)
//! into an in-memory snapshot, which the ordinary vault code then reads (DESIGN §9).

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use needle_vault::Vault;
use needle_vault::store::Snapshot;

use crate::github::{self, Creds};

/// Whether the phone needs a file to show the vault. Scenes, notes and settings are text; the
/// cut bin, kept clashes and anything else are left on GitHub.
fn wanted(path: &str) -> bool {
    let text = path.ends_with(".md") || path.ends_with(".toml");
    let skipped = path.starts_with(".needle/conflicts/") || path.contains("/cut/") || path.starts_with(".needle/templates/");
    text && !skipped
}

/// The vault as it is on GitHub, and the snapshot it reads and writes through. `progress` hears
/// how many of how many files are in hand.
pub async fn open(creds: &Creds, progress: impl Fn(usize, usize)) -> Result<(Vault, Arc<Snapshot>), String> {
    let files = github::files(creds).await?;
    let wanted: Vec<_> = files.iter().filter(|b| wanted(&b.path)).cloned().collect();
    let total = wanted.len();
    progress(0, total);
    let contents = github::contents(creds, &wanted, |done| progress(done, total)).await?;

    let snapshot = Arc::new(Snapshot::new(now()));
    for blob in &files {
        snapshot.add_path(&blob.path, &blob.sha);
    }
    for (blob, text) in contents {
        snapshot.add_contents(&blob.path, text.into_bytes());
    }
    let vault = Vault::open_on(snapshot.clone(), Path::new("")).map_err(|e| match e {
        needle_vault::Error::NotFound(_) | needle_vault::Error::Io(_) => {
            "This repository isn't a Needle and Thread vault: it has no .needle/vault.toml.".to_owned()
        }
        e => e.to_string(),
    })?;
    Ok((vault, snapshot))
}

/// wasm has no clock of its own, so the time comes from JavaScript.
pub fn now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_millis(js_sys::Date::now() as u64)
}

#[cfg(test)]
mod tests {
    use super::wanted;

    #[test]
    fn only_the_files_the_phone_reads_are_fetched() {
        assert!(wanted(".needle/vault.toml"));
        assert!(wanted("projects/tidewater/manuscript/night-market.md"));
        assert!(wanted("worlds/aster/notes/characters/mara.md"));
        assert!(!wanted(".needle/conflicts/2026-10-10-1016/projects/tidewater/manuscript/night-market.md"));
        assert!(!wanted("projects/tidewater/cut/2026-10-07-passage.md"));
        assert!(!wanted("photo.jpg"));
    }
}
