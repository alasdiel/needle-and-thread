// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod history;
mod scenes;
mod spelling;

use tauri::{Manager, RunEvent};

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle();
            scenes::seed_samples(handle)?;
            let history = history::open(handle, scenes::spike_vault(handle)?)?;
            app.manage(history);
            history::start_timer(handle.clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            scenes::sample_scenes,
            scenes::open_scene,
            scenes::save_scene,
            spelling::spell_dictionary,
            spelling::add_to_dictionary,
            history::scene_history,
            history::scene_version,
            history::snapshot_now,
            history::restore_version,
            history::name_version,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Needle and Thread")
        .run(|app, event| {
            // Whatever was saved since the last snapshot goes into one more before quitting.
            if let RunEvent::Exit = event {
                let history = app.state::<history::History>();
                if history.has_changes()
                    && let Err(e) = history.snapshot(app, None)
                {
                    eprintln!("final snapshot failed: {e}");
                }
            }
        });
}
