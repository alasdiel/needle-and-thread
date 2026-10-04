//! Line icons on a 24-unit grid, drawn in the current text colour.
//!
//! Spool, Needle, Basket and Thread are our own. The rest come from Lucide (ISC), some by way of Feather
//! (MIT); both licences are in `assets/licenses/lucide-LICENSE.txt`, which ships with the app.

use leptos::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Back,
    Basket,
    Book,
    Bookmark,
    Check,
    ChevronDown,
    ChevronRight,
    Close,
    File,
    Flag,
    Folder,
    History,
    Link,
    Mail,
    MapPin,
    Monitor,
    Moon,
    More,
    Needle,
    Person,
    Plus,
    Restore,
    Scissors,
    Search,
    Settings,
    Spool,
    Sun,
    Thread,
}

impl Glyph {
    fn paths(self) -> &'static str {
        match self {
            Self::Back => r#"<path d="m12 19-7-7 7-7"/><path d="M19 12H5"/>"#,
            Self::Basket => {
                r#"<path d="M4 10h16l-1.5 9.2a1 1 0 0 1-1 .8h-11a1 1 0 0 1-1-.8L4 10Z"/><path d="M8.5 10 11 4"/><path d="M15.5 10 13 4"/><path d="M9 14v3"/><path d="M12 14v3"/><path d="M15 14v3"/>"#
            }
            Self::Book => {
                r#"<path d="M12 7v14"/><path d="M3 18a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h5a4 4 0 0 1 4 4 4 4 0 0 1 4-4h5a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-6a3 3 0 0 0-3 3 3 3 0 0 0-3-3z"/>"#
            }
            Self::Bookmark => r#"<path d="m19 21-7-4-7 4V5a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v16z"/>"#,
            Self::Check => r#"<path d="M20 6 9 17l-5-5"/>"#,
            Self::ChevronDown => r#"<path d="m6 9 6 6 6-6"/>"#,
            Self::ChevronRight => r#"<path d="m9 18 6-6-6-6"/>"#,
            Self::Close => r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#,
            Self::File => {
                r#"<path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/><path d="M16 13H8"/><path d="M16 17H8"/>"#
            }
            Self::Flag => {
                r#"<path d="M4 22V4a1 1 0 0 1 .4-.8A6 6 0 0 1 8 2c3 0 5 2 7.333 2q2 0 3.067-.8A1 1 0 0 1 20 4v10a1 1 0 0 1-.4.8A6 6 0 0 1 16 16c-3 0-5-2-8-2a6 6 0 0 0-4 1.528"/>"#
            }
            Self::Folder => {
                r#"<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>"#
            }
            Self::History => r#"<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/><path d="M12 7v5l4 2"/>"#,
            Self::Link => {
                r#"<path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/>"#
            }
            Self::Mail => r#"<rect width="20" height="16" x="2" y="4" rx="2"/><path d="m22 7-8.97 5.7a1.94 1.94 0 0 1-2.06 0L2 7"/>"#,
            Self::MapPin => {
                r#"<path d="M20 10c0 4.993-5.539 10.193-7.399 11.799a1 1 0 0 1-1.202 0C9.539 20.193 4 14.993 4 10a8 8 0 0 1 16 0"/><circle cx="12" cy="10" r="3"/>"#
            }
            Self::Monitor => r#"<rect width="20" height="14" x="2" y="3" rx="2"/><path d="M8 21h8"/><path d="M12 17v4"/>"#,
            Self::Moon => r#"<path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/>"#,
            Self::More => {
                r#"<circle cx="5" cy="12" r="1" fill="currentColor"/><circle cx="12" cy="12" r="1" fill="currentColor"/><circle cx="19" cy="12" r="1" fill="currentColor"/>"#
            }
            Self::Needle => {
                r#"<path d="M4.5 19.5 16 8"/><ellipse cx="18" cy="6" rx="1.3" ry="3" transform="rotate(45 18 6)"/><path d="M18.8 5.2c3 1.5 1.5 6-2.5 7.5S8.5 15 9.5 19"/>"#
            }
            Self::Person => r#"<circle cx="12" cy="8" r="5"/><path d="M20 21a8 8 0 0 0-16 0"/>"#,
            Self::Plus => r#"<path d="M5 12h14"/><path d="M12 5v14"/>"#,
            Self::Restore => r#"<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/>"#,
            Self::Scissors => {
                r#"<circle cx="6" cy="6" r="3"/><path d="M8.12 8.12 12 12"/><path d="M20 4 8.12 15.88"/><circle cx="6" cy="18" r="3"/><path d="M14.8 14.8 20 20"/>"#
            }
            Self::Search => r#"<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>"#,
            Self::Settings => {
                r#"<path d="M21 4h-7"/><path d="M10 4H3"/><path d="M21 12h-9"/><path d="M8 12H3"/><path d="M21 20h-5"/><path d="M12 20H3"/><path d="M14 2v4"/><path d="M8 10v4"/><path d="M16 18v4"/>"#
            }
            Self::Spool => {
                r#"<rect x="4.5" y="2.5" width="15" height="3" rx="1.5"/><rect x="4.5" y="18.5" width="15" height="3" rx="1.5"/><path d="M7 5.5v13"/><path d="M17 5.5v13"/><path d="m7 9.5 10-2"/><path d="m7 13 10-2"/><path d="m7 16.5 10-2"/>"#
            }
            Self::Thread => r#"<path d="M2 14c2.5-5 5.5-5 8 0s5.5 5 8 0c1-2 2.2-3 4-3"/>"#,
            Self::Sun => {
                r#"<circle cx="12" cy="12" r="4"/><path d="M12 2v2"/><path d="M12 20v2"/><path d="m4.93 4.93 1.41 1.41"/><path d="m17.66 17.66 1.41 1.41"/><path d="M2 12h2"/><path d="M20 12h2"/><path d="m6.34 17.66-1.41 1.41"/><path d="m19.07 4.93-1.41 1.41"/>"#
            }
        }
    }
}

/// A decorative icon: the label or title next to it carries the meaning.
#[component]
pub fn Icon(glyph: Glyph, #[prop(default = 16)] size: u32) -> impl IntoView {
    view! {
        <svg
            class="icon"
            width=size.to_string()
            height=size.to_string()
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="1.7"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
            inner_html=glyph.paths()
        ></svg>
    }
}
