//! Sharing (DESIGN §10): what a link is, how its text and comments are sealed, and what the
//! sealed text says. The desktop app, the reader page and the Worker all use this, so they
//! agree on every byte. Nothing here makes random numbers: callers pass them in, since each
//! place gets them its own way (the OS, or the browser's crypto).
//!
//! A link is `<reader>#<id>.<key>`. Browsers never send what follows `#`, so the Worker sees
//! the id but never the key, and only ever stores what the key has sealed.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

use crate::anchor::Anchor;

/// Bytes of random in a share's id: enough that nobody guesses one.
pub const ID_BYTES: usize = 16;
pub const KEY_BYTES: usize = 32;
pub const NONCE_BYTES: usize = 12;

/// What a reader's link holds: which share, and the key that opens it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub id: String,
    pub key: [u8; KEY_BYTES],
}

impl Link {
    /// A new link from fresh random bytes.
    pub fn new(id: [u8; ID_BYTES], key: [u8; KEY_BYTES]) -> Self {
        Self { id: B64.encode(id), key }
    }

    /// What goes after `#`: `<id>.<key>`.
    pub fn fragment(&self) -> String {
        format!("{}.{}", self.id, B64.encode(self.key))
    }

    /// What goes after `#` for the author: `<id>.<key>.<author key>`, which adds the author's
    /// tools to the reader page.
    pub fn author_fragment(&self, author: &str) -> String {
        format!("{}.{author}", self.fragment())
    }

    /// Reads `<id>.<key>`, with or without the `#`.
    pub fn parse(fragment: &str) -> Option<Self> {
        match Self::parse_author(fragment)? {
            (link, None) => Some(link),
            _ => None,
        }
    }

    /// Reads `<id>.<key>` or the author's `<id>.<key>.<author key>`.
    pub fn parse_author(fragment: &str) -> Option<(Self, Option<String>)> {
        let mut parts = fragment.trim_start_matches('#').split('.');
        let (id, key, author) = (parts.next()?, parts.next()?, parts.next());
        if parts.next().is_some() || author.is_some_and(|a| B64.decode(a).is_err()) {
            return None;
        }
        let key: [u8; KEY_BYTES] = B64.decode(key).ok()?.try_into().ok()?;
        valid_id(id).then(|| (Self { id: id.to_owned(), key }, author.map(str::to_owned)))
    }

    /// What lets someone with the link add a comment: derived from the key, so the Worker can
    /// check it (by its hash) without being able to read anything.
    pub fn comment_token(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(b"needle-and-thread comment token\0");
        hash.update(self.key);
        B64.encode(hash.finalize())
    }
}

/// Whether `id` could be a share's id: base64url, the right length. The Worker checks this
/// before using it as a storage key.
pub fn valid_id(id: &str) -> bool {
    B64.decode(id).is_ok_and(|bytes| bytes.len() == ID_BYTES)
}

/// Bytes as base64url, the way the Worker keeps sealed comments.
pub fn encode(bytes: &[u8]) -> String {
    B64.encode(bytes)
}

pub fn decode(text: &str) -> Option<Vec<u8>> {
    B64.decode(text).ok()
}

/// A secret as the Worker keeps it: its SHA-256, hex.
pub fn fingerprint(secret: &str) -> String {
    Sha256::digest(secret.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// A random secret as text, e.g. the author key: base64url of the bytes.
pub fn secret(bytes: [u8; 32]) -> String {
    B64.encode(bytes)
}

/// Seals `plain` with `key`, as `nonce ‖ ciphertext`. `what` is bound in too (it isn't
/// stored), so a sealed comment can't be passed off as the text, or one share's as another's.
pub fn seal(key: &[u8; KEY_BYTES], nonce: [u8; NONCE_BYTES], what: &str, plain: &[u8]) -> Vec<u8> {
    let cipher = Aes256Gcm::new(key.into());
    let sealed = cipher
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: plain, aad: what.as_bytes() })
        .expect("AES-GCM encrypts anything shorter than 64 GiB");
    [nonce.as_slice(), &sealed].concat()
}

/// Opens what `seal` made, or None if it isn't that (a wrong key, or tampered with).
pub fn open(key: &[u8; KEY_BYTES], what: &str, sealed: &[u8]) -> Option<Vec<u8>> {
    if sealed.len() < NONCE_BYTES {
        return None;
    }
    let (nonce, body) = sealed.split_at(NONCE_BYTES);
    Aes256Gcm::new(key.into()).decrypt(Nonce::from_slice(nonce), Payload { msg: body, aad: what.as_bytes() }).ok()
}

/// What's bound into sealing a share's text.
pub fn text_label(id: &str) -> String {
    format!("needle-and-thread text {id}")
}

/// What's bound into sealing a comment on a share.
pub fn comment_label(id: &str) -> String {
    format!("needle-and-thread comment {id}")
}

/// The shared text, before it's sealed: a scene, a chapter, a part or the whole manuscript,
/// as the desktop app put it together. Only prose; notes are never shared.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shared {
    /// The format, for readers of an older or newer one.
    pub v: u32,
    /// The project's title.
    pub project: String,
    /// What was shared: "The night market", "Chapter 1: Arrival", "The whole manuscript".
    pub title: String,
    pub parts: Vec<SharedPart>,
    /// When this version was made, in seconds since the Unix epoch.
    pub updated: i64,
    /// The snapshot it was made from, which comments made on it record.
    #[serde(default)]
    pub version: String,
    /// Where comments are now, by their id, as the author's app last placed them as the text
    /// changed; None for one whose words are gone. A comment not here is where it was made.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub moved: BTreeMap<u64, Option<On>>,
}

/// Where a comment is: a scene, and words in it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct On {
    pub scene: String,
    #[serde(flatten)]
    pub anchor: Anchor,
}

/// A comment as whoever wrote it sealed it. The Worker adds its id, its time, whether the author
/// posted it, and whether it's resolved or deleted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    pub v: u32,
    /// The name its writer gave; empty for the author.
    #[serde(default)]
    pub name: String,
    pub text: String,
    /// The words it's on; none for a reply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<On>,
    /// The version of the shared text it was made on.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
    /// The comment this answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<u64>,
}

/// Longest a comment's text may be, in characters: well inside what the Worker takes once sealed.
pub const MAX_COMMENT_CHARS: usize = 4000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedPart {
    /// Where a new chapter (or part) starts, its heading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chapter: Option<String>,
    /// The scene's name in `manuscript/`, which comments refer to.
    pub scene: String,
    pub title: String,
    pub markdown: String,
}

pub const FORMAT: u32 = 1;

/// What a link shares: one scene, a chapter, a part (a run of chapters with the same part
/// label) or the whole book.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "covers", rename_all = "lowercase")]
pub enum Covers {
    Scene { scene: String },
    Chapter { chapter: String },
    Part { part: String },
    Book,
}

/// A link the author has made, as the vault keeps it in `.needle/shares.toml`. The keys are
/// here so every device of the author's can upload, moderate and revoke; the vault is private.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareRecord {
    pub id: String,
    /// The link's key, base64url.
    pub key: String,
    /// The author key.
    pub author: String,
    /// What the author calls it: "Beta readers".
    pub name: String,
    /// The project's folder name.
    pub project: String,
    #[serde(flatten)]
    pub covers: Covers,
    /// When it was made, in seconds since the Unix epoch.
    pub created: i64,
}

impl ShareRecord {
    /// A new link, from fresh random bytes for its id, its key and its author key.
    pub fn new(random: [u8; ID_BYTES + KEY_BYTES + 32], name: &str, project: &str, covers: Covers, created: i64) -> Self {
        let id: [u8; ID_BYTES] = random[..ID_BYTES].try_into().expect("ID_BYTES long");
        let key: [u8; KEY_BYTES] = random[ID_BYTES..ID_BYTES + KEY_BYTES].try_into().expect("KEY_BYTES long");
        let author: [u8; 32] = random[ID_BYTES + KEY_BYTES..].try_into().expect("32 long");
        let link = Link::new(id, key);
        Self { id: link.id, key: B64.encode(key), author: secret(author), name: name.trim().to_owned(), project: project.to_owned(), covers, created }
    }

    pub fn link(&self) -> Option<Link> {
        Link::parse(&format!("{}.{}", self.id, self.key))
    }
}

#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareList {
    #[serde(default, rename = "share")]
    pub shares: Vec<ShareRecord>,
}

impl ShareList {
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| format!("shares.toml: {e}"))
    }

    pub fn to_toml(&self) -> String {
        let body = toml::to_string(self).expect("shares serialise");
        format!("# Links made with Share. Each key here can read or moderate what it shares: keep this vault private.\n\n{body}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; KEY_BYTES] = [7; KEY_BYTES];

    #[test]
    fn a_link_reads_back() {
        let link = Link::new([1; ID_BYTES], KEY);
        let fragment = link.fragment();
        assert!(!fragment.contains(['+', '/', '=']), "safe in a URL: {fragment}");
        assert_eq!(Link::parse(&fragment), Some(link.clone()));
        assert_eq!(Link::parse(&format!("#{fragment}")), Some(link));
        assert_eq!(Link::parse("nonsense"), None);
        assert_eq!(Link::parse("AAAA.AAAA"), None);
    }

    #[test]
    fn sealed_text_opens_only_with_its_key_and_label() {
        let sealed = seal(&KEY, [3; NONCE_BYTES], &text_label("a"), b"The market opened at dusk.");
        assert_eq!(open(&KEY, &text_label("a"), &sealed).as_deref(), Some(b"The market opened at dusk.".as_slice()));
        assert_eq!(open(&[8; KEY_BYTES], &text_label("a"), &sealed), None);
        assert_eq!(open(&KEY, &comment_label("a"), &sealed), None, "a text can't pass as a comment");
        assert_eq!(open(&KEY, &text_label("b"), &sealed), None, "or as another share's");
        let mut tampered = sealed.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert_eq!(open(&KEY, &text_label("a"), &tampered), None);
    }

    #[test]
    fn the_comment_token_comes_from_the_key_alone() {
        let a = Link::new([1; ID_BYTES], KEY);
        let b = Link::new([2; ID_BYTES], KEY);
        assert_eq!(a.comment_token(), b.comment_token());
        assert_ne!(a.comment_token(), Link::new([1; ID_BYTES], [8; KEY_BYTES]).comment_token());
        assert_eq!(fingerprint("abc").len(), 64);
    }

    #[test]
    fn the_shared_text_round_trips_as_json() {
        let shared = Shared {
            v: FORMAT,
            project: "Tidewater".into(),
            title: "Arrival".into(),
            parts: vec![SharedPart { chapter: Some("Arrival".into()), scene: "night-market".into(), title: "The night market".into(), markdown: "Text.\n".into() }],
            updated: 1_791_627_360,
            version: "abc123".into(),
            moved: BTreeMap::from([(4, None), (7, Some(On { scene: "night-market".into(), anchor: Anchor { quote: "Text".into(), n: 0, before: String::new(), after: ".".into() } }))]),
        };
        let json = serde_json::to_string(&shared).unwrap();
        assert_eq!(serde_json::from_str::<Shared>(&json).unwrap(), shared);
    }

    #[test]
    fn the_author_link_carries_the_author_key() {
        let link = Link::new([1; ID_BYTES], KEY);
        let author = secret([9; 32]);
        let fragment = link.author_fragment(&author);
        assert_eq!(Link::parse_author(&fragment), Some((link.clone(), Some(author))));
        assert_eq!(Link::parse_author(&link.fragment()), Some((link.clone(), None)));
        assert_eq!(Link::parse(&fragment), None, "a reader's link has no author key");
        assert_eq!(Link::parse_author(&format!("{fragment}.more")), None);
    }

    #[test]
    fn the_vaults_list_of_links_round_trips() {
        let record = ShareRecord::new([5; ID_BYTES + KEY_BYTES + 32], " Beta readers ", "tidewater", Covers::Chapter { chapter: "arrival".into() }, 1_791_627_360);
        assert_eq!(record.name, "Beta readers");
        assert!(record.link().is_some());
        let list = ShareList { shares: vec![record, ShareRecord::new([6; ID_BYTES + KEY_BYTES + 32], "Editor", "tidewater", Covers::Book, 1)] };
        let text = list.to_toml();
        assert!(text.contains("covers = \"chapter\"") && text.contains("chapter = \"arrival\"") && text.contains("covers = \"book\""), "{text}");
        assert_eq!(ShareList::parse(&text).unwrap(), list);
        assert_eq!(ShareList::parse("").unwrap(), ShareList::default());
    }

    #[test]
    fn a_comment_round_trips_as_json() {
        let comment = Comment {
            v: FORMAT,
            name: "Ines".into(),
            text: "Why before?".into(),
            on: Some(On { scene: "night-market".into(), anchor: Anchor { quote: "lit before the bells".into(), n: 0, before: "were ".into(), after: ".".into() } }),
            version: "abc123".into(),
            reply_to: None,
        };
        let json = serde_json::to_string(&comment).unwrap();
        assert!(json.contains(r#""scene":"night-market","quote":"lit before the bells""#), "{json}");
        assert_eq!(serde_json::from_str::<Comment>(&json).unwrap(), comment);
    }
}
