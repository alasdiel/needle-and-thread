// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod closing;
mod history;
mod notes;
mod search;
mod settings;
mod spelling;
mod state;
mod workspace;

use needle_vault::Vault;
use tauri::{Manager, RunEvent, WindowEvent};

use closing::Closing;
use state::AppState;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .manage(Closing::default())
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
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                closing::requested(window, api);
            }
        })
        .invoke_handler(tauri::generate_handler![
            workspace::current_vault,
            workspace::open_vault,
            workspace::create_vault,
            workspace::open_sample_vault,
            workspace::set_typography,
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
            notes::project_notes,
            notes::open_note,
            notes::save_note,
            notes::create_note,
            notes::rename_note,
            notes::set_note_aliases,
            notes::note_links,
            notes::link_mention,
            notes::promote_note,
            notes::cut_note,
            notes::project_settings,
            notes::update_project,
            notes::scene_names,
            notes::set_scene_names,
            search::search,
            spelling::spell_dictionary,
            spelling::add_to_dictionary,
            history::scene_history,
            history::scene_version,
            history::note_history,
            history::note_version,
            history::snapshot_now,
            history::restore_version,
            history::restore_note_version,
            history::name_version,
            settings::appearance,
            settings::set_appearance,
            settings::markdown_panel,
            settings::set_markdown_panel,
            closing::close_listening,
            closing::finish_close,
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
