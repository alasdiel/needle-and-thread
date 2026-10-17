//! What the reader page shows, worked out from the shared text and its comments: threads, where
//! each one is in the text now, and the marks for a scene.

use needle_core::anchor::{Anchor, prose};
use needle_core::share::{Comment, On, Shared};

use crate::render::Mark;

/// A comment, opened, with what the Worker knows of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Opened {
    pub id: u64,
    pub author: bool,
    pub resolved: bool,
    /// Seconds since the Unix epoch.
    pub created: f64,
    pub comment: Comment,
}

impl Opened {
    /// Who wrote it, as the page says it.
    pub fn who(&self) -> String {
        if self.author || self.comment.name.trim().is_empty() { "Author".to_owned() } else { self.comment.name.trim().to_owned() }
    }
}

/// A comment on some words and its replies.
#[derive(Clone, Debug, PartialEq)]
pub struct Thread {
    pub head: Opened,
    /// Where its words are now; None if the author has since cut them.
    pub at: Option<On>,
    pub replies: Vec<Opened>,
}

impl Thread {
    /// The words it was made on.
    pub fn quote(&self) -> &str {
        self.head.comment.on.as_ref().map_or("", |on| on.anchor.quote.as_str())
    }
}

/// Every thread, in reading order: by scene, then by where in it; ones whose words are gone last.
pub fn threads(shared: &Shared, comments: &[Opened]) -> Vec<Thread> {
    let order = |scene: &str| shared.parts.iter().position(|p| p.scene == scene).unwrap_or(usize::MAX);
    let mut threads: Vec<(usize, usize, Thread)> = comments
        .iter()
        .filter(|c| c.comment.reply_to.is_none() && c.comment.on.is_some())
        .map(|head| {
            let at = match shared.moved.get(&head.id) {
                Some(moved) => moved.clone(),
                None => head.comment.on.clone(),
            };
            let at = at.filter(|on| order(&on.scene) != usize::MAX);
            let (scene, offset) = match &at {
                Some(on) => {
                    let part = &shared.parts[order(&on.scene)];
                    (order(&on.scene), place(&prose(&part.markdown), &on.anchor).map_or(usize::MAX, |r| r.start))
                }
                None => (usize::MAX, usize::MAX),
            };
            let mut replies: Vec<Opened> = comments.iter().filter(|c| c.comment.reply_to == Some(head.id)).cloned().collect();
            replies.sort_by(|a, b| a.created.total_cmp(&b.created));
            (scene, offset, Thread { head: head.clone(), at, replies })
        })
        .collect();
    threads.sort_by_key(|(scene, offset, t)| (*scene, *offset, t.head.id));
    threads.into_iter().map(|(_, _, t)| t).collect()
}

/// Where an anchor's words are in a scene's prose: exactly, else wherever the quote went.
fn place(prose: &str, anchor: &Anchor) -> Option<std::ops::Range<usize>> {
    anchor.find(prose).or_else(|| anchor.search(prose))
}

/// The marks for one scene: its unresolved threads' words.
pub fn marks(markdown: &str, scene: &str, threads: &[Thread]) -> Vec<Mark> {
    let prose = prose(markdown);
    threads
        .iter()
        .filter(|t| !t.head.resolved)
        .filter_map(|t| {
            let on = t.at.as_ref().filter(|on| on.scene == scene)?;
            Some(Mark { range: place(&prose, &on.anchor)?, id: t.head.id })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use needle_core::share::{FORMAT, SharedPart};

    fn shared() -> Shared {
        Shared {
            v: FORMAT,
            project: "Tidewater".into(),
            title: "Arrival".into(),
            parts: vec![
                SharedPart { chapter: Some("Arrival".into()), scene: "harbor".into(), title: "The harbor".into(), markdown: "The tide was out.\n".into() },
                SharedPart { chapter: None, scene: "market".into(), title: "The night market".into(), markdown: "The lanterns were lit before the bells.\n".into() },
            ],
            updated: 0,
            version: "v2".into(),
            moved: Default::default(),
        }
    }

    fn comment(id: u64, scene: &str, quote: &str, reply_to: Option<u64>) -> Opened {
        let on = (!scene.is_empty()).then(|| On { scene: scene.into(), anchor: Anchor { quote: quote.into(), n: 0, before: String::new(), after: String::new() } });
        Opened { id, author: false, resolved: false, created: id as f64, comment: Comment { v: FORMAT, name: "Ines".into(), text: "Hm.".into(), on, version: "v1".into(), reply_to } }
    }

    #[test]
    fn threads_come_in_reading_order_with_their_replies() {
        let mut s = shared();
        let comments = vec![comment(1, "market", "the bells", None), comment(2, "harbor", "tide", None), comment(3, "", "", Some(1)), comment(4, "market", "lanterns", None), comment(5, "market", "gone words", None)];
        s.moved.insert(5, None);
        let threads = threads(&s, &comments);
        let order: Vec<u64> = threads.iter().map(|t| t.head.id).collect();
        assert_eq!(order, [2, 4, 1, 5]);
        assert_eq!(threads[2].replies.iter().map(|r| r.id).collect::<Vec<_>>(), [3]);
        assert_eq!(threads[3].at, None, "its words were cut");
        assert_eq!(threads[3].quote(), "gone words");
    }

    #[test]
    fn a_scene_marks_its_unresolved_threads_where_they_are_now() {
        let mut s = shared();
        let mut resolved = comment(4, "market", "lanterns", None);
        resolved.resolved = true;
        let comments = vec![comment(1, "market", "lit before", None), resolved, comment(2, "harbor", "tide", None)];
        // The author's app moved comment 1 onto the words as they are now.
        s.moved.insert(1, Some(On { scene: "market".into(), anchor: Anchor { quote: "the bells".into(), n: 0, before: String::new(), after: String::new() } }));
        let threads = threads(&s, &comments);
        assert_eq!(marks(&s.parts[1].markdown, "market", &threads), [Mark { range: 29..38, id: 1 }]);
        assert_eq!(comments[0].who(), "Ines");
    }
}
