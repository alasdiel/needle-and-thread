// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod history;
mod spelling;
mod state;
mod workspace;

use needle_vault::Vault;
use tauri::{Manager, RunEvent};

use state::AppState;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .setup(|app| {
            let handle = app.handle();
            // NEEDLE_VAULT opens a given vault (handy in development); otherwise last time's
            // vault reopens, and if it's gone the app starts at the welcome screen.
            let from_env = std::env::var_os("NEEDLE_VAULT").map(std::path::PathBuf::from);
            let remember = from_env.is_none();
            if let Some(root) = from_env.or_else(|| state::last_vault(handle)) {
                match Vault::open(&root) {
                    Ok(vault) => app.state::<AppState>().open(handle, vault, remember)?,
                    Err(e) => eprintln!("couldn't reopen {}: {e}", root.display()),
                }
            }
            history::start_timer(handle.clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            workspace::current_vault,
            workspace::open_vault,
            workspace::create_vault,
            workspace::open_sample_vault,
            workspace::create_project,
            workspace::project_outline,
            workspace::open_scene,
            workspace::save_scene,
            workspace::create_scene,
            workspace::rename_scene,
            workspace::set_scene_status,
            workspace::cut_scene,
            workspace::split_scene,
            workspace::merge_scene,
            workspace::move_scene,
            workspace::add_chapter,
            workspace::rename_chapter,
            workspace::set_chapter_part,
            workspace::move_chapter,
            workspace::remove_chapter,
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
            if let RunEvent::Exit = event
                && let Some(open) = app.state::<AppState>().lock().as_ref()
                && open.history.has_changes()
                && let Err(e) = open.history.snapshot(app, None)
            {
                eprintln!("final snapshot failed: {e}");
            }
        });
}
