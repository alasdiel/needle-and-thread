//! The phone app (DESIGN §9): the vault read straight from its GitHub repository, in a browser
//! added to the home screen.

mod app;
mod github;
mod load;
mod model;
mod render;
mod search;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
