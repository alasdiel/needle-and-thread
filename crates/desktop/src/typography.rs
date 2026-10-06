//! Settings panel for the automatic typography changes, one switch each, saved in the vault.

use leptos::{prelude::*, task::spawn_local};
use needle_core::settings::TypographyRule;

use crate::editor::Typography;
use crate::tauri;

struct Rule {
    rule: TypographyRule,
    label: &'static str,
    example: &'static str,
    /// Only for what the example doesn't already show.
    note: Option<&'static str>,
}

const RULES: [Rule; 4] = [
    Rule {
        rule: TypographyRule::DoubleQuotes,
        label: "Curly double quotes",
        example: "\"Hi\" → “Hi”",
        note: None,
    },
    Rule {
        rule: TypographyRule::SingleQuotes,
        label: "Curly single quotes",
        example: "'it's' → ‘it’s’",
        note: Some("Includes apostrophes."),
    },
    Rule {
        rule: TypographyRule::EmDash,
        label: "Em dash",
        example: "-- → —",
        note: Some("--- on its own line is still a scene break."),
    },
    Rule {
        rule: TypographyRule::Ellipsis,
        label: "Ellipsis",
        example: "... → …",
        note: None,
    },
];

#[component]
pub fn TypographySettings(typography: RwSignal<Typography>) -> impl IntoView {
    let error = RwSignal::new(None::<String>);
    let switch = move |rule: TypographyRule, on: bool| {
        typography.update(|t| t.set(rule, on));
        // The panel may have closed by the time the save finishes.
        spawn_local(async move { error.try_set(tauri::set_typography(rule, on).await.err()); });
    };
    let rules = RULES
        .iter()
        .map(|rule| {
            view! {
                <label class="setting">
                    <span class="setting-text">
                        <span class="setting-label">{rule.label}</span>
                        <code class="setting-example">{rule.example}</code>
                        {rule.note.map(|note| view! { <span class="setting-description">{note}</span> })}
                    </span>
                    <input
                        type="checkbox"
                        class="switch"
                        prop:checked=move || typography.get().is_on(rule.rule)
                        on:change=move |ev| switch(rule.rule, event_target_checked(&ev))
                    />
                </label>
            }
        })
        .collect_view();

    view! {
        <div class="panel" role="dialog" aria-label="Typography">
            <h2>"As you type"</h2>
            {rules}
            {move || error.get().map(|e| view! { <p class="error">{format!("Couldn't save this: {e}")}</p> })}
        </div>
    }
}
