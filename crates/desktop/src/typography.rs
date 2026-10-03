//! Settings panel for the automatic typography changes, one switch each.

use leptos::prelude::*;

use crate::editor::Typography;

struct Rule {
    label: &'static str,
    example: &'static str,
    /// Only for what the example doesn't already show.
    note: Option<&'static str>,
    get: fn(&Typography) -> bool,
    set: fn(&mut Typography, bool),
}

const RULES: [Rule; 4] = [
    Rule {
        label: "Curly double quotes",
        example: "\"Hi\" → “Hi”",
        note: None,
        get: |t| t.double_quotes,
        set: |t, on| t.double_quotes = on,
    },
    Rule {
        label: "Curly single quotes",
        example: "'it's' → ‘it’s’",
        note: Some("Includes apostrophes."),
        get: |t| t.single_quotes,
        set: |t, on| t.single_quotes = on,
    },
    Rule {
        label: "Em dash",
        example: "-- → —",
        note: Some("--- on its own line is still a scene break."),
        get: |t| t.em_dash,
        set: |t, on| t.em_dash = on,
    },
    Rule {
        label: "Ellipsis",
        example: "... → …",
        note: None,
        get: |t| t.ellipsis,
        set: |t, on| t.ellipsis = on,
    },
];

#[component]
pub fn TypographySettings(typography: RwSignal<Typography>) -> impl IntoView {
    let rules = RULES
        .iter()
        .map(|rule| {
            view! {
                <label class="setting">
                    <input
                        type="checkbox"
                        prop:checked=move || (rule.get)(&typography.get())
                        on:change=move |ev| typography.update(|t| (rule.set)(t, event_target_checked(&ev)))
                    />
                    <span>
                        <span class="setting-label">{rule.label}</span>
                        <code class="setting-example">{rule.example}</code>
                        {rule.note.map(|note| view! { <span class="setting-description">{note}</span> })}
                    </span>
                </label>
            }
        })
        .collect_view();

    view! {
        <div class="panel" role="dialog" aria-label="Typography">
            <h2>"As you type"</h2>
            {rules}
        </div>
    }
}
