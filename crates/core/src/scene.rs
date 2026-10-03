//! A scene or note file: an optional TOML header between `+++` lines, then the Markdown body.
//!
//! The header is kept as raw text and only rewritten when it actually changes (through
//! [`Header`], which preserves formatting), so saving never touches metadata nobody edited.

use std::fmt;

use crate::header::{Header, HeaderError};

const DELIMITER: &str = "+++";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneFile {
    /// From the opening `+++` line through the closing `+++` line, including its line ending.
    pub front_matter: Option<String>,
    /// Everything after the front matter, exactly as stored.
    pub body: String,
}

impl SceneFile {
    pub fn parse(src: &str) -> Self {
        match front_matter_len(src) {
            Some(len) => Self {
                front_matter: Some(src[..len].to_owned()),
                body: src[len..].to_owned(),
            },
            None => Self {
                front_matter: None,
                body: src.to_owned(),
            },
        }
    }

    /// The body as the editor sees it: without the blank lines that separate it from the
    /// front matter, and with `\n` line endings.
    pub fn markdown(&self) -> String {
        self.body[self.leading_blank_len()..].replace("\r\n", "\n")
    }

    /// Replaces the body with editor output, keeping the file's existing layout: the blank
    /// lines after the front matter and its line-ending style. Ends with exactly one newline.
    pub fn set_markdown(&mut self, markdown: &str) {
        let crlf = self.uses_crlf();
        let mut lead = self.body[..self.leading_blank_len()].to_owned();
        if lead.is_empty() && self.front_matter.is_some() && self.body.trim().is_empty() {
            lead.push_str(if crlf { "\r\n" } else { "\n" });
        }

        let content = markdown.trim_end_matches(['\n', '\r']);
        let mut body = lead;
        if !content.is_empty() {
            if crlf {
                body.push_str(&content.replace("\r\n", "\n").replace('\n', "\r\n"));
                body.push_str("\r\n");
            } else {
                body.push_str(content);
                body.push('\n');
            }
        }
        self.body = body;
    }

    /// The parsed header; empty if the file has none.
    pub fn header(&self) -> Result<Header, HeaderError> {
        match &self.front_matter {
            Some(front_matter) => Header::parse(&header_toml(front_matter).replace("\r\n", "\n")),
            None => Ok(Header::new()),
        }
    }

    /// Writes `header` back, keeping the file's line endings. Leaves the file untouched if the
    /// header text is unchanged.
    pub fn set_header(&mut self, header: &Header) {
        let crlf = self.uses_crlf();
        let mut toml = header.to_string();
        if !toml.is_empty() && !toml.ends_with('\n') {
            toml.push('\n');
        }
        let mut front_matter = format!("{DELIMITER}\n{toml}{DELIMITER}\n");
        if crlf {
            front_matter = front_matter.replace('\n', "\r\n");
        }
        if self.front_matter.as_deref() == Some(front_matter.as_str()) {
            return;
        }
        if self.front_matter.is_none() && !self.body.is_empty() && !self.body.starts_with(['\n', '\r']) {
            self.body.insert_str(0, if crlf { "\r\n" } else { "\n" });
        }
        self.front_matter = Some(front_matter);
    }

    fn leading_blank_len(&self) -> usize {
        let mut len = 0;
        for line in self.body.split_inclusive('\n') {
            if !line.trim().is_empty() || !line.ends_with('\n') {
                break;
            }
            len += line.len();
        }
        len
    }

    fn uses_crlf(&self) -> bool {
        let first_line_ending = |s: &str| s.find('\n').map(|i| i > 0 && s.as_bytes()[i - 1] == b'\r');
        self.front_matter
            .as_deref()
            .and_then(first_line_ending)
            .or_else(|| first_line_ending(&self.body))
            .unwrap_or(false)
    }
}

impl fmt::Display for SceneFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(front_matter) = &self.front_matter {
            f.write_str(front_matter)?;
        }
        f.write_str(&self.body)
    }
}

/// The TOML between the delimiter lines of a front-matter block.
fn header_toml(front_matter: &str) -> &str {
    let start = front_matter.find('\n').map_or(front_matter.len(), |i| i + 1);
    let end = front_matter.trim_end_matches(['\n', '\r']).rfind('\n').map_or(start, |i| i + 1);
    &front_matter[start..end.max(start)]
}

/// Byte length of a complete front-matter block at the start of `src`, if there is one.
/// An opening `+++` without a closing one means the file has no front matter.
fn front_matter_len(src: &str) -> Option<usize> {
    let first_end = src.find('\n')? + 1;
    if !is_delimiter(&src[..first_end]) {
        return None;
    }
    let mut pos = first_end;
    for line in src[first_end..].split_inclusive('\n') {
        pos += line.len();
        if is_delimiter(line) {
            return Some(pos);
        }
    }
    None
}

fn is_delimiter(line: &str) -> bool {
    line.trim_end() == DELIMITER
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCENE: &str = "+++\nid = \"sc_1\"\ntitle = \"The night market\"\n+++\n\nThe market opened at dusk.\n";

    #[test]
    fn splits_front_matter_from_body() {
        let scene = SceneFile::parse(SCENE);
        assert_eq!(
            scene.front_matter.as_deref(),
            Some("+++\nid = \"sc_1\"\ntitle = \"The night market\"\n+++\n")
        );
        assert_eq!(scene.body, "\nThe market opened at dusk.\n");
        assert_eq!(scene.markdown(), "The market opened at dusk.\n");
    }

    #[test]
    fn round_trips_any_input_unchanged() {
        let inputs = [
            SCENE,
            "",
            "+++",
            "+++\n",
            "No front matter.\n",
            "+++\nunterminated = true\n\nBody text.\n",
            "+++\na = 1\n+++",
            "+++\r\na = 1\r\n+++\r\n\r\nWindows text.\r\n",
            "+++\na = 1\n+++\n\nBefore.\n\n+++\n\nAfter.\n",
            "  +++\nnot = \"front matter\"\n+++\n",
            "---\n\nA scene that starts with a scene break.\n\n---\n\nAnd has another.\n",
        ];
        for input in inputs {
            assert_eq!(SceneFile::parse(input).to_string(), input, "input: {input:?}");
        }
    }

    #[test]
    fn scene_breaks_are_never_front_matter() {
        let scene = SceneFile::parse("---\n\nOpening.\n\n---\n\nLater.\n");
        assert_eq!(scene.front_matter, None);
    }

    #[test]
    fn unterminated_front_matter_is_body() {
        let scene = SceneFile::parse("+++\nunterminated = true\n\nBody.\n");
        assert_eq!(scene.front_matter, None);
    }

    #[test]
    fn delimiter_in_body_does_not_end_front_matter_early() {
        let scene = SceneFile::parse("+++\na = 1\n+++\n\nBefore.\n\n+++\n\nAfter.\n");
        assert_eq!(scene.front_matter.as_deref(), Some("+++\na = 1\n+++\n"));
        assert_eq!(scene.markdown(), "Before.\n\n+++\n\nAfter.\n");
    }

    #[test]
    fn setting_unchanged_markdown_keeps_file_identical() {
        for input in [SCENE, "Just a body.\n", "+++\na = 1\n+++\n\n\nTwo blank lines kept.\n"] {
            let mut scene = SceneFile::parse(input);
            scene.set_markdown(&scene.markdown());
            assert_eq!(scene.to_string(), input);
        }
    }

    #[test]
    fn set_markdown_keeps_front_matter_and_separator() {
        let mut scene = SceneFile::parse(SCENE);
        scene.set_markdown("The market closed early.");
        assert_eq!(
            scene.to_string(),
            "+++\nid = \"sc_1\"\ntitle = \"The night market\"\n+++\n\nThe market closed early.\n"
        );
    }

    #[test]
    fn set_markdown_adds_separator_to_empty_body() {
        let mut scene = SceneFile::parse("+++\na = 1\n+++\n");
        scene.set_markdown("First words.\n");
        assert_eq!(scene.to_string(), "+++\na = 1\n+++\n\nFirst words.\n");
    }

    #[test]
    fn set_markdown_keeps_crlf_files_crlf() {
        let mut scene = SceneFile::parse("+++\r\na = 1\r\n+++\r\n\r\nOld.\r\n");
        assert_eq!(scene.markdown(), "Old.\n");
        scene.set_markdown("New.\n\nSecond paragraph.\n");
        assert_eq!(scene.to_string(), "+++\r\na = 1\r\n+++\r\n\r\nNew.\r\n\r\nSecond paragraph.\r\n");
    }

    #[test]
    fn clearing_the_body_leaves_front_matter() {
        let mut scene = SceneFile::parse(SCENE);
        scene.set_markdown("");
        assert_eq!(scene.to_string(), "+++\nid = \"sc_1\"\ntitle = \"The night market\"\n+++\n\n");
    }

    #[test]
    fn reads_and_edits_the_header() {
        let mut scene = SceneFile::parse(SCENE);
        let mut header = scene.header().unwrap();
        assert_eq!(header.title(), Some("The night market"));
        header.set_str("status", "draft");
        scene.set_header(&header);
        assert_eq!(
            scene.to_string(),
            "+++\nid = \"sc_1\"\ntitle = \"The night market\"\nstatus = \"draft\"\n+++\n\nThe market opened at dusk.\n"
        );
    }

    #[test]
    fn unchanged_header_leaves_file_identical() {
        let mut scene = SceneFile::parse("+++\r\nid = \"sc_1\"   # keep me\r\n+++\r\n\r\nText.\r\n");
        let before = scene.to_string();
        let header = scene.header().unwrap();
        scene.set_header(&header);
        assert_eq!(scene.to_string(), before);
    }

    #[test]
    fn adding_a_header_to_a_plain_file() {
        let mut scene = SceneFile::parse("Just prose.\n");
        let mut header = Header::new();
        header.set_str("id", "sc_2");
        scene.set_header(&header);
        assert_eq!(scene.to_string(), "+++\nid = \"sc_2\"\n+++\n\nJust prose.\n");
        assert_eq!(scene.markdown(), "Just prose.\n");
    }

    #[test]
    fn empty_header_block_parses() {
        let scene = SceneFile::parse("+++\n+++\n\nText.\n");
        assert_eq!(scene.header().unwrap().to_string(), "");
    }
}
