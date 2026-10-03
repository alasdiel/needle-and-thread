//! Line icons on a 24-unit grid, drawn in the current text colour.
//!
//! Monitor and Moon come from Lucide by way of Feather (MIT); Settings and Sun from Lucide (ISC).
//! Both licences are in `assets/licenses/lucide-LICENSE.txt`, which ships with the app.

use leptos::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Monitor,
    Moon,
    Settings,
    Sun,
}

impl Glyph {
    fn paths(self) -> &'static str {
        match self {
            Self::Monitor => r#"<rect width="20" height="14" x="2" y="3" rx="2"/><path d="M8 21h8"/><path d="M12 17v4"/>"#,
            Self::Moon => r#"<path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/>"#,
            Self::Settings => {
                r#"<path d="M21 4h-7"/><path d="M10 4H3"/><path d="M21 12h-9"/><path d="M8 12H3"/><path d="M21 20h-5"/><path d="M12 20H3"/><path d="M14 2v4"/><path d="M8 10v4"/><path d="M16 18v4"/>"#
            }
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
