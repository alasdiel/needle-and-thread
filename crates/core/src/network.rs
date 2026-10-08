//! The network board's layout (`network.toml` in a project): where each note's card is pinned,
//! the zones drawn under them, and the marks the writer has added (DESIGN §7).
//!
//! Nothing here decides what's *on* the board. Nodes and relationships come from the notes
//! themselves; this file only remembers how they've been arranged. A note with no entry has
//! simply never been placed, and a stale entry for a note that's gone is harmless, so the app
//! never has to repair this file after a note is renamed or deleted.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A point on the board, in board units (the same units the map is drawn in before zoom).
pub type Point = (f64, f64);

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Network {
    /// Where each note's card sits, by note id. The board's own origin is wherever the cards
    /// happen to be: there's no fixed top left.
    pub nodes: BTreeMap<String, Node>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub zones: Vec<Zone>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub marks: Vec<Mark>,
}

/// A pinned card. `turn` is how far it's tilted, in degrees; the board keeps these small.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub at: Point,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub turn: f64,
}

/// A sheet of kraft paper pinned under a group of cards, drawn and named by the writer. It
/// means nothing to the rest of the app: cards aren't "in" a zone, they just sit over it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Zone {
    pub id: String,
    pub name: String,
    pub at: Point,
    pub size: Point,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub turn: f64,
}

/// Something the writer put on the board themselves. Handwriting only ever appears here: the
/// app writes nothing by hand (DESIGN §7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Mark {
    /// A handwritten note, left loose on the board or dropped onto a card.
    Note {
        id: String,
        text: String,
        at: Point,
        #[serde(default, skip_serializing_if = "is_zero")]
        turn: f64,
        /// The note id of the card this travels with, if it was dropped on one. `at` is then
        /// measured from that card rather than the board.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        on: Option<String>,
    },
    /// A marker loop drawn around a group.
    Ring {
        id: String,
        at: Point,
        size: Point,
        #[serde(default, skip_serializing_if = "is_zero")]
        turn: f64,
    },
    /// A marker arrow drawn between two places on the board. `bend` curves it, so two arrows
    /// between the same cards don't lie on top of each other.
    Arrow {
        id: String,
        from: Point,
        to: Point,
        #[serde(default, skip_serializing_if = "is_zero")]
        bend: f64,
    },
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde's skip_serializing_if hands us a reference
fn is_zero(value: &f64) -> bool {
    *value == 0.0
}

impl Mark {
    pub fn id(&self) -> &str {
        match self {
            Self::Note { id, .. } | Self::Ring { id, .. } | Self::Arrow { id, .. } => id,
        }
    }

    /// The same mark, picked up and put down `by` further along. An arrow moves both ends; a
    /// note keeps the card it's on, since its spot is measured from that card.
    pub fn shifted(&self, by: Point) -> Self {
        let add = |p: Point| (p.0 + by.0, p.1 + by.1);
        let mut mark = self.clone();
        match &mut mark {
            Self::Note { at, .. } | Self::Ring { at, .. } => *at = add(*at),
            Self::Arrow { from, to, .. } => (*from, *to) = (add(*from), add(*to)),
        }
        mark
    }
}

/// A marker loop around a box centred on `at`, as SVG path data. Drawn by hand, it doesn't
/// close neatly: it starts a little early, wobbles, and runs on past where it began.
pub fn ring_path(at: Point, size: Point) -> String {
    let (rx, ry) = (size.0.abs() / 2.0, size.1.abs() / 2.0);
    let start = -0.5_f64;
    let end = start + std::f64::consts::TAU + 0.55;
    let steps = 48;
    let mut d = String::new();
    for i in 0..=steps {
        let t = start + (end - start) * f64::from(i) / f64::from(steps);
        // The loop swells a touch in three places and comes back a little wider than it set
        // out, which is what a quick pen stroke does.
        let wobble = 1.0 + 0.035 * (3.0 * t).sin() + 0.05 * (t - start) / (end - start);
        let (x, y) = (at.0 + rx * wobble * t.cos(), at.1 + ry * wobble * t.sin());
        d.push_str(&format!("{}{x:.1},{y:.1}", if i == 0 { "M" } else { " L" }));
    }
    d
}

/// A marker arrow from `from` to `to`, bowed sideways by `bend`: the shaft, and the two short
/// strokes of its head, as SVG path data.
pub fn arrow_path(from: Point, to: Point, bend: f64) -> (String, String) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let len = dx.hypot(dy).max(1.0);
    // The bow is sideways to the line, at its middle.
    let (nx, ny) = (-dy / len, dx / len);
    let (cx, cy) = ((from.0 + to.0) / 2.0 + nx * bend, (from.1 + to.1) / 2.0 + ny * bend);
    let shaft = format!("M{:.1},{:.1} Q{cx:.1},{cy:.1} {:.1},{:.1}", from.0, from.1, to.0, to.1);
    // The head points along the curve as it arrives, which is from the bow's peak to the tip.
    let (hx, hy) = (to.0 - cx, to.1 - cy);
    let angle = hy.atan2(hx);
    let reach = 14.0_f64.min(len / 3.0);
    let barb = |turn: f64| {
        let a = angle + std::f64::consts::PI + turn;
        (to.0 + reach * a.cos(), to.1 + reach * a.sin())
    };
    let ((ax, ay), (bx, by)) = (barb(0.45), barb(-0.45));
    let head = format!("M{ax:.1},{ay:.1} L{:.1},{:.1} L{bx:.1},{by:.1}", to.0, to.1);
    (shaft, head)
}

impl Network {
    pub fn parse(toml: &str) -> Result<Self, String> {
        toml::from_str(toml).map_err(|e| format!("network.toml isn't valid: {}", e.message()))
    }

    pub fn to_toml(&self) -> String {
        // Not to_string_pretty: it breaks every point over four lines.
        let body = toml::to_string(self).expect("network layout serializes");
        format!("{HEADER}{body}")
    }

    /// Where a new card goes: near the middle of whatever it links to, nudged off anything
    /// already there. Keeps new notes from piling up on one spot without laying the board out
    /// for the writer, who arranges it themselves.
    pub fn room_for(&self, near: &[String]) -> Point {
        let placed: Vec<Point> = near
            .iter()
            .filter_map(|id| self.nodes.get(id))
            .map(|n| n.at)
            .collect();
        let (x, y) = match placed.len() {
            0 => (0.0, 0.0),
            n => (
                placed.iter().map(|p| p.0).sum::<f64>() / n as f64,
                placed.iter().map(|p| p.1).sum::<f64>() / n as f64 + BELOW,
            ),
        };
        self.free_spot(x, y)
    }

    /// Spirals outward from (x, y) until it finds a spot no card is sitting on.
    fn free_spot(&self, x: f64, y: f64) -> Point {
        let taken = |at: Point| {
            self.nodes
                .values()
                .any(|n| (n.at.0 - at.0).abs() < APART.0 && (n.at.1 - at.1).abs() < APART.1)
        };
        if !taken((x, y)) {
            return (x, y);
        }
        for ring in 1..32 {
            let step = ring as f64;
            for (dx, dy) in [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0), (1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
                let at = (x + dx * step * APART.0, y + dy * step * APART.1);
                if !taken(at) {
                    return at;
                }
            }
        }
        (x, y)
    }
}

/// How far below its links a new card starts, and how far apart cards must be to count as not
/// overlapping. A card is at most 136 × 116.
const BELOW: f64 = 168.0;
const APART: (f64, f64) = (152.0, 132.0);

const HEADER: &str = "\
# How this project's network board is arranged: where each card is pinned, the zones drawn
# under them, and the marks you've added. Yours to edit, though it's easier on the board.
#
# Cards are keyed by note id. An id that no longer exists is ignored, and a note with no
# entry here just hasn't been placed yet.

";

#[cfg(test)]
mod tests {
    use super::*;

    fn node(x: f64, y: f64) -> Node {
        Node { at: (x, y), turn: 0.0 }
    }

    #[test]
    fn shifting_a_mark_moves_every_point_of_it() {
        let arrow = Mark::Arrow { id: "mk_1".into(), from: (0.0, 0.0), to: (100.0, 50.0), bend: 12.0 };
        assert_eq!(
            arrow.shifted((10.0, -5.0)),
            Mark::Arrow { id: "mk_1".into(), from: (10.0, -5.0), to: (110.0, 45.0), bend: 12.0 }
        );
        let note = Mark::Note { id: "mk_2".into(), text: "hm".into(), at: (5.0, 5.0), turn: 0.0, on: Some("nt_1".into()) };
        let Mark::Note { at, on, .. } = note.shifted((1.0, 2.0)) else { unreachable!() };
        assert_eq!((at, on.as_deref()), ((6.0, 7.0), Some("nt_1")), "stays on its card");
    }

    /// Pulls the numbers out of SVG path data, as (x, y) pairs.
    fn points(d: &str) -> Vec<(f64, f64)> {
        let nums: Vec<f64> = d
            .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
            .filter(|s| !s.is_empty())
            .map(|s| s.parse().unwrap())
            .collect();
        nums.chunks(2).map(|p| (p[0], p[1])).collect()
    }

    #[test]
    fn a_ring_goes_round_its_box_and_overshoots() {
        let pts = points(&ring_path((100.0, 50.0), (80.0, 40.0)));
        // Every point is near the ellipse: inside 1.15 of its radii, outside 0.85.
        for (x, y) in &pts {
            let r = (((x - 100.0) / 40.0).powi(2) + ((y - 50.0) / 20.0).powi(2)).sqrt();
            assert!((0.85..1.15).contains(&r), "({x}, {y}) is {r} radii out");
        }
        // It runs on past where it started, so the two ends sit apart, not on top of each other.
        let (first, last) = (pts[0], pts[pts.len() - 1]);
        assert!((first.0 - last.0).hypot(first.1 - last.1) > 5.0, "{first:?} {last:?}");
    }

    #[test]
    fn an_arrow_ends_where_it_points_and_its_head_trails_behind() {
        let (shaft, head) = arrow_path((0.0, 0.0), (200.0, 0.0), 0.0);
        assert_eq!(points(&shaft).last(), Some(&(200.0, 0.0)));
        let barbs = points(&head);
        assert_eq!(barbs[1], (200.0, 0.0), "the head's point is the tip");
        // A straight arrow pointing right has its barbs behind the tip, one either side.
        assert!(barbs[0].0 < 200.0 && barbs[2].0 < 200.0);
        assert!(barbs[0].1 * barbs[2].1 < 0.0, "{barbs:?}");
    }

    #[test]
    fn a_bent_arrow_bows_to_one_side() {
        let (shaft, _) = arrow_path((0.0, 0.0), (200.0, 0.0), 30.0);
        let control = points(&shaft)[1];
        assert_eq!(control.0, 100.0);
        assert!(control.1.abs() > 20.0, "{control:?}");
    }

    #[test]
    fn empty_file_is_an_empty_board() {
        let net = Network::parse("").unwrap();
        assert!(net.nodes.is_empty() && net.zones.is_empty() && net.marks.is_empty());
    }

    #[test]
    fn round_trips_through_toml() {
        let mut net = Network::default();
        net.nodes.insert("nt_1".into(), Node { at: (120.0, 240.0), turn: -1.5 });
        net.nodes.insert("nt_2".into(), node(300.0, 240.0));
        net.zones.push(Zone {
            id: "zn_1".into(),
            name: "The harbour".into(),
            at: (40.0, 60.0),
            size: (520.0, 380.0),
            turn: -0.6,
        });
        net.marks.push(Mark::Note {
            id: "mk_1".into(),
            text: "who has it now?".into(),
            at: (600.0, 120.0),
            turn: -6.0,
            on: Some("nt_2".into()),
        });
        net.marks.push(Mark::Ring { id: "mk_2".into(), at: (572.0, 396.0), size: (84.0, 54.0), turn: -6.0 });
        net.marks.push(Mark::Arrow { id: "mk_3".into(), from: (452.0, 150.0), to: (690.0, 96.0), bend: 30.0 });

        let text = net.to_toml();
        assert!(text.starts_with("# How this project's network board"));
        assert_eq!(Network::parse(&text).unwrap(), net);
    }

    #[test]
    fn a_plain_board_writes_no_empty_tables() {
        let mut net = Network::default();
        net.nodes.insert("nt_1".into(), node(0.0, 0.0));
        let text = net.to_toml();
        // The words themselves are in the file's own header comment, so look for the tables.
        assert!(!text.contains("[[zones]]"), "{text}");
        assert!(!text.contains("[[marks]]"), "{text}");
        assert!(!text.contains("turn"), "an untilted card writes no turn: {text}");
    }

    #[test]
    fn unknown_fields_and_ids_are_left_alone() {
        // A newer version's mark kind is the one thing we can't keep, but it mustn't crash.
        let net = Network::parse(
            r#"
            [nodes]
            "nt_gone" = { at = [10.0, 20.0] }
            "#,
        )
        .unwrap();
        assert_eq!(net.nodes["nt_gone"].at, (10.0, 20.0));
    }

    #[test]
    fn a_first_card_lands_at_the_origin() {
        assert_eq!(Network::default().room_for(&[]), (0.0, 0.0));
    }

    #[test]
    fn a_new_card_goes_below_what_it_links_to() {
        let mut net = Network::default();
        net.nodes.insert("nt_a".into(), node(0.0, 0.0));
        net.nodes.insert("nt_b".into(), node(200.0, 100.0));
        let at = net.room_for(&["nt_a".into(), "nt_b".into()]);
        assert_eq!(at.0, 100.0, "midway between the two");
        assert!(at.1 >= 50.0 + BELOW, "below them: {at:?}");
    }

    #[test]
    fn a_new_card_avoids_one_already_there() {
        let mut net = Network::default();
        net.nodes.insert("nt_a".into(), node(0.0, 0.0));
        // Right where a card linked to nt_a would land.
        net.nodes.insert("nt_b".into(), node(0.0, BELOW));
        let at = net.room_for(&["nt_a".into()]);
        assert_ne!(at, (0.0, BELOW));
        assert!(net.nodes.values().all(|n| (n.at.0 - at.0).abs() >= APART.0 || (n.at.1 - at.1).abs() >= APART.1));
    }

    #[test]
    fn links_that_arent_placed_yet_are_ignored() {
        let mut net = Network::default();
        net.nodes.insert("nt_a".into(), node(50.0, 50.0));
        assert_eq!(net.room_for(&["nt_a".into(), "nt_never_placed".into()]), (50.0, 50.0 + BELOW));
    }
}
