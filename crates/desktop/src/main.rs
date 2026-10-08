mod app;
mod appearance;
mod backup;
mod bin;
mod closing;
mod editor;
mod envelope;
mod history;
mod icons;
mod links;
mod network_spike;
mod network;
mod notes;
mod outline;
mod pattern;
mod project;
mod search;
mod settings;
mod spell;
mod status;
mod tauri;
mod timeline;
mod typography;
mod welcome;
mod workspace;

fn main() {
    console_error_panic_hook::set_once();
    if let Some((count, layer)) = network_spike::wanted() {
        leptos::mount::mount_to_body(move || leptos::view! { <network_spike::NetworkSpike count layer /> });
        return;
    }
    leptos::mount::mount_to_body(app::App);
}
