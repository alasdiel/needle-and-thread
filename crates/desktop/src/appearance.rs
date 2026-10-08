//! System, Light or Dark. The choice sets `data-theme` on the page, which picks the colours in
//! `styles/app.css`; with System the attribute is left off and the desktop's setting decides.

use serde::Deserialize;

use crate::icons::Glyph;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

impl Appearance {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    /// How the backend stores it, and the value of `data-theme`.
    pub fn name(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    pub fn glyph(self) -> Glyph {
        match self {
            Self::System => Glyph::Monitor,
            Self::Light => Glyph::Sun,
            Self::Dark => Glyph::Moon,
        }
    }

    /// Shows the whole page in this appearance, and remembers it for the next first paint.
    pub fn apply(self) {
        let Some(window) = web_sys::window() else { return };
        if let Some(root) = window.document().and_then(|d| d.document_element()) {
            let _ = match self {
                Self::System => root.remove_attribute("data-theme"),
                _ => root.set_attribute("data-theme", self.name()),
            };
        }
        // `index.html` reads this before the page paints, so the window opens in the right
        // theme instead of flashing the desktop's. The backend is still what's saved.
        if let Ok(Some(store)) = window.local_storage() {
            let _ = match self {
                Self::System => store.remove_item("needle-theme"),
                _ => store.set_item("needle-theme", self.name()),
            };
        }
    }
}
