//! Markings printed on a sewing-pattern piece. They decorate the app's frame only; the
//! `.pattern-piece` styles in `styles/app.css` draw the cutting and stitching lines.

use leptos::prelude::*;

/// The small triangles on a piece's edges, which on paper show where pieces join.
#[component]
pub fn Notches() -> impl IntoView {
    view! {
        <span class="notch left top" aria-hidden="true"></span>
        <span class="notch right top" aria-hidden="true"></span>
        <span class="notch left bottom" aria-hidden="true"></span>
        <span class="notch right bottom" aria-hidden="true"></span>
    }
}

/// The double-headed arrow showing which way the fabric's grain runs.
#[component]
pub fn Grainline() -> impl IntoView {
    view! {
        <span class="grainline" aria-hidden="true">
            <GrainArrow />
            <span>"Grainline"</span>
        </span>
    }
}

// Its own component so the <svg> is a view's root: nested inside the span, its drawing didn't
// show up.
#[component]
fn GrainArrow() -> impl IntoView {
    view! {
        <svg
            width="12"
            height="320"
            viewBox="0 0 12 320"
            fill="none"
            stroke="currentColor"
            stroke-width="1.2"
            stroke-linejoin="round"
            inner_html=r#"<path d="M6 3v314"/><path d="M1.5 12 6 2l4.5 10"/><path d="M1.5 308 6 318l4.5-10"/>"#
        ></svg>
    }
}
