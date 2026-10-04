//! Which note a name points to, for `[[links]]` and for names in scene headers (`pov`, `cast`…).
//!
//! A name is looked up in the project first, then in the project's world; in each, titles come
//! before aliases. Case, spacing and apostrophe style don't matter. `[[tidewater/Mara Venn]]`
//! reaches the project in folder `tidewater`, whichever project the link is in. A name that
//! fits two notes equally well is ambiguous rather than silently picking one.

use std::collections::HashMap;

use crate::links::name_key;

/// Where a note lives: a project or a world, by folder name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Owner {
    Project(String),
    World(String),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Resolution<'a, T> {
    Found(&'a T),
    /// Two or more notes share the name at the same level, e.g. an alias used twice.
    Ambiguous(Vec<&'a T>),
    Missing,
}

/// Looking names up from inside one project.
#[derive(Debug, Clone)]
pub struct NameIndex<T> {
    project: String,
    world: Option<String>,
    items: Vec<T>,
    /// Checked in order, first match wins: this project's titles, its aliases, then the
    /// world's titles and aliases.
    tiers: [HashMap<String, Vec<usize>>; 4],
    /// `project/name` for every project, titles before aliases.
    qualified: [HashMap<String, Vec<usize>>; 2],
}

impl<T> NameIndex<T> {
    /// An empty index for links written in `project` (a folder name), which belongs to `world`.
    pub fn new(project: &str, world: Option<&str>) -> Self {
        Self {
            project: project.to_owned(),
            world: world.map(str::to_owned),
            items: Vec::new(),
            tiers: Default::default(),
            qualified: Default::default(),
        }
    }

    /// Adds a note. Notes from other worlds are ignored, since no link here can reach them.
    pub fn add(&mut self, owner: &Owner, title: &str, aliases: &[String], item: T) {
        let base = match owner {
            Owner::Project(p) if *p == self.project => Some(0),
            Owner::World(w) if Some(w) == self.world.as_ref() => Some(2),
            Owner::Project(_) => None,
            Owner::World(_) => return,
        };
        let index = self.items.len();
        self.items.push(item);
        let insert = |map: &mut HashMap<String, Vec<usize>>, key: String| {
            let entry = map.entry(key).or_default();
            if !entry.contains(&index) {
                entry.push(index);
            }
        };
        let names = || std::iter::once((0, title)).chain(aliases.iter().map(|a| (1, a.as_str())));
        for (tier, name) in names() {
            let key = name_key(name);
            if key.is_empty() {
                continue;
            }
            if let Some(base) = base {
                insert(&mut self.tiers[base + tier], key.clone());
            }
            if let Owner::Project(p) = owner {
                insert(&mut self.qualified[tier], format!("{}/{key}", name_key(p)));
            }
        }
    }

    pub fn resolve(&self, name: &str) -> Resolution<'_, T> {
        let key = name_key(name);
        let qualified = key.contains('/').then_some(&self.qualified);
        for map in qualified.into_iter().flatten().chain(&self.tiers) {
            match map.get(&key).map(Vec::as_slice) {
                None | Some([]) => continue,
                Some([one]) => return Resolution::Found(&self.items[*one]),
                Some(many) => return Resolution::Ambiguous(many.iter().map(|i| &self.items[*i]).collect()),
            }
        }
        Resolution::Missing
    }

    /// Every note added, in the order added.
    pub fn items(&self) -> &[T] {
        &self.items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(slug: &str) -> Owner {
        Owner::Project(slug.into())
    }

    fn world(slug: &str) -> Owner {
        Owner::World(slug.into())
    }

    fn aliases(names: &[&str]) -> Vec<String> {
        names.iter().map(|&n| n.to_owned()).collect()
    }

    fn index() -> NameIndex<&'static str> {
        let mut index = NameIndex::new("tidewater", Some("glass-coast"));
        index.add(&project("tidewater"), "Mara Venn", &aliases(&["Mara", "the Captain"]), "mara");
        index.add(&project("tidewater"), "Old Teodor", &[], "teodor");
        index.add(&world("glass-coast"), "Mara", &[], "grandmother");
        index.add(&world("glass-coast"), "The Drowning", &aliases(&["the Flood"]), "drowning");
        index.add(&world("other-world"), "Ys", &[], "ys");
        index.add(&project("saltmarsh"), "Mara Venn", &[], "older mara");
        index
    }

    #[test]
    fn titles_and_aliases_ignoring_case() {
        let index = index();
        assert_eq!(index.resolve("Mara Venn"), Resolution::Found(&"mara"));
        assert_eq!(index.resolve("old  TEODOR"), Resolution::Found(&"teodor"));
        assert_eq!(index.resolve("The Captain"), Resolution::Found(&"mara"));
        assert_eq!(index.resolve("Nobody"), Resolution::Missing);
    }

    #[test]
    fn the_project_comes_before_its_world() {
        let index = index();
        // The project's alias beats the world's title.
        assert_eq!(index.resolve("Mara"), Resolution::Found(&"mara"));
        assert_eq!(index.resolve("the flood"), Resolution::Found(&"drowning"));
        assert_eq!(index.resolve("Ys"), Resolution::Missing);
    }

    #[test]
    fn a_project_prefix_reaches_that_project() {
        let index = index();
        assert_eq!(index.resolve("saltmarsh/Mara Venn"), Resolution::Found(&"older mara"));
        assert_eq!(index.resolve("Tidewater/mara"), Resolution::Found(&"mara"));
        // Other projects stay out of plain lookups.
        assert_eq!(index.resolve("Mara Venn"), Resolution::Found(&"mara"));
    }

    #[test]
    fn a_slash_that_is_not_a_project_is_part_of_the_name() {
        let mut index = NameIndex::new("tidewater", None);
        index.add(&project("tidewater"), "Either/Or", &[], "either");
        assert_eq!(index.resolve("either/or"), Resolution::Found(&"either"));
    }

    #[test]
    fn shared_names_are_ambiguous() {
        let mut index = NameIndex::new("tidewater", None);
        index.add(&project("tidewater"), "Mara Venn", &aliases(&["Venn"]), "mara");
        index.add(&project("tidewater"), "Joss Venn", &aliases(&["Venn"]), "joss");
        index.add(&project("tidewater"), "Joss", &[], "joss note");
        assert_eq!(index.resolve("Venn"), Resolution::Ambiguous(vec![&"mara", &"joss"]));
        // A title still beats another note's alias.
        index.add(&project("tidewater"), "Teodor", &aliases(&["Joss"]), "teodor");
        assert_eq!(index.resolve("Joss"), Resolution::Found(&"joss note"));
    }

    #[test]
    fn a_note_listing_its_title_as_an_alias_is_not_ambiguous() {
        let mut index = NameIndex::new("tidewater", None);
        index.add(&project("tidewater"), "Mara", &aliases(&["mara", "MARA"]), "mara");
        assert_eq!(index.resolve("Mara"), Resolution::Found(&"mara"));
    }
}
