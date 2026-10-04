mod app;
mod appearance;
mod editor;
mod envelope;
mod history;
mod icons;
mod links;
mod notes;
mod outline;
mod pattern;
mod project;
mod search;
mod settings;
mod spell;
mod status;
mod tauri;
mod typography;
mod welcome;
mod workspace;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
