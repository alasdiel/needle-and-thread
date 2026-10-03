// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod scenes;
mod spelling;

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            scenes::sample_scenes,
            scenes::open_scene,
            scenes::save_scene,
            spelling::spell_dictionary,
            spelling::add_to_dictionary,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Needle and Thread");
}
