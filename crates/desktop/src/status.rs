//! Status marks: a ring that fills as a scene moves through the statuses in `vault.toml`, empty
//! for the first and full for the last, so any number of statuses works without colours to tell
//! apart. A status the list doesn't have is drawn dashed.

use std::f64::consts::TAU;

use leptos::prelude::*;

/// How far through `statuses` a scene with `status` is, from 0 to 1; None if it isn't listed.
pub fn progress(status: &str, statuses: &[String]) -> Option<f64> {
    let index = statuses.iter().position(|s| s == status)?;
    Some(match statuses.len() {
        1 => 1.0,
        n => index as f64 / (n - 1) as f64,
    })
}

/// The ring and its fill, on a 12-unit grid.
fn markup(progress: Option<f64>) -> String {
    let ring = |extra: &str| format!(r#"<circle cx="6" cy="6" r="5" fill="none" stroke="currentColor" stroke-width="1.3"{extra}/>"#);
    match progress {
        None => ring(r#" stroke-dasharray="2 1.6""#),
        Some(p) if p <= 0.0 => ring(""),
        Some(p) if p >= 1.0 => ring("") + r#"<circle cx="6" cy="6" r="3.5" fill="currentColor"/>"#,
        Some(p) => {
            // A wedge from twelve o'clock, clockwise.
            let angle = p * TAU;
            let (x, y) = (6.0 + 3.5 * angle.sin(), 6.0 - 3.5 * angle.cos());
            let large = u8::from(p > 0.5);
            ring("") + &format!(r#"<path d="M6 6V2.5A3.5 3.5 0 {large} 1 {x:.2} {y:.2}Z" fill="currentColor"/>"#)
        }
    }
}

#[component]
pub fn StatusMark(progress: Option<f64>) -> impl IntoView {
    view! {
        <svg class="status-mark" width="13" height="13" viewBox="0 0 12 12" aria-hidden="true" inner_html=markup(progress)></svg>
    }
}
