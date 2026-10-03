//! A project's reading order, stored in `outline.toml`: a list of chapters, each holding scene
//! file names. Parts are a label on chapters, so consecutive chapters with the same `part` form
//! a part. Scene files never move; restructuring only edits this list.

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outline {
    #[serde(default, rename = "chapter")]
    pub chapters: Vec<Chapter>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chapter {
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    /// Scene file names in `manuscript/`, without `.md`.
    #[serde(default)]
    pub scenes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutlineError {
    Invalid(String),
    NoSuchChapter(String),
    NoSuchScene(String),
}

impl fmt::Display for OutlineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(e) => write!(f, "outline.toml isn't valid: {e}"),
            Self::NoSuchChapter(id) => write!(f, "no chapter with id {id}"),
            Self::NoSuchScene(slug) => write!(f, "{slug} isn't in the outline"),
        }
    }
}

impl std::error::Error for OutlineError {}

const PREAMBLE: &str = "# Reading order. Written by Needle and Thread; scene names refer to files in manuscript/.\n\n";

impl Outline {
    pub fn parse(toml: &str) -> Result<Self, OutlineError> {
        toml::from_str(toml).map_err(|e| OutlineError::Invalid(e.message().to_owned()))
    }

    pub fn to_toml(&self) -> String {
        // Serializing plain strings, vectors and options can't fail.
        let body = toml::to_string(self).expect("outline serializes");
        format!("{PREAMBLE}{body}")
    }

    /// Scene names in reading order.
    pub fn scenes(&self) -> impl Iterator<Item = &str> {
        self.chapters.iter().flat_map(|c| c.scenes.iter().map(String::as_str))
    }

    pub fn chapter(&self, id: &str) -> Option<&Chapter> {
        self.chapters.iter().find(|c| c.id == id)
    }

    /// The chapter (by index) and position of a scene.
    pub fn locate(&self, scene: &str) -> Option<(usize, usize)> {
        self.chapters
            .iter()
            .enumerate()
            .find_map(|(c, chapter)| chapter.scenes.iter().position(|s| s == scene).map(|i| (c, i)))
    }

    /// Inserts `scene` into chapter `chapter_id` at `index` (clamped to the end).
    pub fn insert_scene(&mut self, chapter_id: &str, index: usize, scene: &str) -> Result<(), OutlineError> {
        let chapter = self.chapter_mut(chapter_id)?;
        let index = index.min(chapter.scenes.len());
        chapter.scenes.insert(index, scene.to_owned());
        Ok(())
    }

    /// Inserts `scene` right after `after` in the same chapter.
    pub fn insert_scene_after(&mut self, after: &str, scene: &str) -> Result<(), OutlineError> {
        let (c, i) = self.locate(after).ok_or_else(|| OutlineError::NoSuchScene(after.to_owned()))?;
        self.chapters[c].scenes.insert(i + 1, scene.to_owned());
        Ok(())
    }

    pub fn remove_scene(&mut self, scene: &str) -> bool {
        match self.locate(scene) {
            Some((c, i)) => {
                self.chapters[c].scenes.remove(i);
                true
            }
            None => false,
        }
    }

    /// Moves `scene` to `index` in chapter `chapter_id`, where `index` counts positions in the
    /// target chapter as it is before the move.
    pub fn move_scene(&mut self, scene: &str, chapter_id: &str, index: usize) -> Result<(), OutlineError> {
        let (from_c, from_i) = self.locate(scene).ok_or_else(|| OutlineError::NoSuchScene(scene.to_owned()))?;
        let to_c = self.chapter_index(chapter_id)?;
        let index = if from_c == to_c && from_i < index { index - 1 } else { index };
        let moved = self.chapters[from_c].scenes.remove(from_i);
        let target = &mut self.chapters[to_c].scenes;
        target.insert(index.min(target.len()), moved);
        Ok(())
    }

    pub fn insert_chapter(&mut self, index: usize, chapter: Chapter) {
        let index = index.min(self.chapters.len());
        self.chapters.insert(index, chapter);
    }

    /// Moves a chapter to `index`, counted as in [`Outline::move_scene`].
    pub fn move_chapter(&mut self, id: &str, index: usize) -> Result<(), OutlineError> {
        let from = self.chapter_index(id)?;
        let index = if from < index { index - 1 } else { index };
        let chapter = self.chapters.remove(from);
        self.insert_chapter(index, chapter);
        Ok(())
    }

    /// Removes an empty chapter. A chapter that still has scenes is refused, so no scene is
    /// ever dropped from the outline by accident.
    pub fn remove_chapter(&mut self, id: &str) -> Result<Chapter, OutlineError> {
        let index = self.chapter_index(id)?;
        if !self.chapters[index].scenes.is_empty() {
            return Err(OutlineError::Invalid(format!("chapter {id} still has scenes")));
        }
        Ok(self.chapters.remove(index))
    }

    pub fn chapter_mut(&mut self, id: &str) -> Result<&mut Chapter, OutlineError> {
        self.chapters
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(|| OutlineError::NoSuchChapter(id.to_owned()))
    }

    fn chapter_index(&self, id: &str) -> Result<usize, OutlineError> {
        self.chapters
            .iter()
            .position(|c| c.id == id)
            .ok_or_else(|| OutlineError::NoSuchChapter(id.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outline() -> Outline {
        Outline::parse(
            r#"
[[chapter]]
id = "ol_1"
title = "Arrival"
part = "Part One"
scenes = ["harbor", "market"]

[[chapter]]
id = "ol_2"
title = "The Ledger"
part = "Part One"
summary = "Teodor sells her out."
scenes = ["counting-house"]
"#,
        )
        .unwrap()
    }

    fn order(o: &Outline) -> Vec<&str> {
        o.scenes().collect()
    }

    #[test]
    fn parses_and_lists_reading_order() {
        let o = outline();
        assert_eq!(order(&o), ["harbor", "market", "counting-house"]);
        assert_eq!(o.chapters[1].summary, "Teodor sells her out.");
        assert_eq!(o.locate("market"), Some((0, 1)));
    }

    #[test]
    fn writes_toml_that_reads_back_the_same() {
        let o = outline();
        let text = o.to_toml();
        assert!(text.starts_with("# Reading order."));
        assert_eq!(Outline::parse(&text).unwrap(), o);
    }

    #[test]
    fn moves_scenes_within_and_between_chapters() {
        let mut o = outline();
        o.move_scene("harbor", "ol_1", 2).unwrap();
        assert_eq!(order(&o), ["market", "harbor", "counting-house"]);
        o.move_scene("counting-house", "ol_1", 0).unwrap();
        assert_eq!(order(&o), ["counting-house", "market", "harbor"]);
        assert!(o.chapters[1].scenes.is_empty());
        assert!(o.move_scene("missing", "ol_1", 0).is_err());
        assert!(o.move_scene("market", "ol_9", 0).is_err());
    }

    #[test]
    fn inserts_and_removes_scenes() {
        let mut o = outline();
        o.insert_scene_after("harbor", "night-walk").unwrap();
        o.insert_scene("ol_2", 99, "epilogue").unwrap();
        assert_eq!(order(&o), ["harbor", "night-walk", "market", "counting-house", "epilogue"]);
        assert!(o.remove_scene("night-walk"));
        assert!(!o.remove_scene("night-walk"));
    }

    #[test]
    fn moves_chapters_and_only_removes_empty_ones() {
        let mut o = outline();
        o.move_chapter("ol_2", 0).unwrap();
        assert_eq!(order(&o), ["counting-house", "harbor", "market"]);
        assert!(o.remove_chapter("ol_1").is_err());
        o.insert_chapter(9, Chapter { id: "ol_3".into(), ..Default::default() });
        assert_eq!(o.remove_chapter("ol_3").unwrap().id, "ol_3");
    }

    #[test]
    fn empty_outline_is_valid() {
        assert_eq!(Outline::parse("").unwrap(), Outline::default());
    }
}
