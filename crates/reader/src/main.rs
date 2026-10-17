//! The reader page (DESIGN §10): what a share link opens. The text and comments are sealed with
//! the link's key, which is in the address after `#` and so never reaches a server.

mod app;
mod model;
mod render;
mod worker;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
