//! The main screen: projects with the outline or notes on the left, the open scene (drawn as a
//! pattern piece) or note (a fabric swatch) in the middle, and the History or Markdown panel on
//! the right.

use std::collections::HashMap;
use std::time::Duration;

use leptos::{prelude::*, task::spawn_local};
use needle_core::project::{NoteKind, ProjectKind};
use needle_core::spell::Speller;

use crate::bin::BinPanel;
use crate::closing::BeforeClose;
use crate::editor::{self, Editor, EditorHandle};
use crate::envelope::{self, EnvelopeCard};
use crate::history::{HistoryPanel, HistoryTarget};
use crate::icons::{Glyph, Icon};
use crate::links::{LinkBridge, name_words};
use crate::notes::{self, LinksSwatch, NotesList, Pin};
use crate::outline::{self, LiveWords, OutlineTree};
use crate::pattern::{Grainline, Notches};
use crate::project::ProjectPanel;
use crate::search::{Search, SearchPalette, SearchSidebar};
use crate::settings::{Prefs, SettingsPanel};
use crate::spell::SpellBridge;
use crate::status::{self, StatusMark};
use crate::tauri::{self, CutView, HitView, NoteKey, NoteView, OutlineView, Passage, ProjectView, SceneNamesView, SceneView, VaultView};
use crate::typography::TypographySettings;

#[derive(Clone, PartialEq)]
enum SaveState {
    Saved,
    Saving,
    Failed(String),
}

#[derive(Clone, Copy, PartialEq)]
enum Panel {
    Markdown,
    History,
    Bin,
}

/// A note at the foot of the page: something just went into the cut bin, with Undo, or came
/// back from it somewhere other than its old place.
#[derive(Clone, PartialEq)]
struct Notice {
    text: &'static str,
    /// What Undo puts back.
    undo: Option<CutView>,
    /// Tells notices apart, so an old one's timer doesn't close a newer one.
    id: u32,
}

/// How long a notice stays.
const NOTICE: Duration = Duration::from_secs(8);

/// What the sidebar shows.
#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Outline,
    Notes,
    Search,
}

/// Search words for the editor to highlight.
fn terms_array(terms: &[String]) -> js_sys::Array {
    terms.iter().map(|t| wasm_bindgen::JsValue::from_str(t)).collect()
}

/// Scenes in reading order: chapters first, then unplaced ones.
fn reading_order(outline: &OutlineView) -> Vec<&SceneView> {
    outline.chapters.iter().flat_map(|c| &c.scenes).chain(&outline.unplaced).collect()
}

fn find(outline: &OutlineView, slug: &str) -> Option<SceneView> {
    reading_order(outline).into_iter().find(|s| s.slug == slug).cloned()
}

/// The scene to open after `slug` goes away: the one after it, else the one before.
fn neighbour(outline: &OutlineView, slug: &str) -> Option<String> {
    let order = reading_order(outline);
    let i = order.iter().position(|s| s.slug == slug)?;
    order.get(i + 1).or_else(|| i.checked_sub(1).and_then(|j| order.get(j))).map(|s| s.slug.clone())
}

/// Where a scene sits, for the breadcrumb and the piece's label.
struct Place {
    part: Option<String>,
    /// None for a scene that isn't in the outline.
    chapter: Option<String>,
    /// The scene's number in reading order, from 1.
    number: usize,
}

fn place(outline: &OutlineView, slug: &str) -> Option<Place> {
    let mut number = 0;
    for (index, chapter) in outline.chapters.iter().enumerate() {
        for scene in &chapter.scenes {
            number += 1;
            if scene.slug == slug {
                let title = outline::chapter_title(chapter, index);
                return Some(Place { part: chapter.part.clone(), chapter: Some(title), number });
            }
        }
    }
    let i = outline.unplaced.iter().position(|s| s.slug == slug)?;
    Some(Place { part: None, chapter: None, number: number + i + 1 })
}

fn kind_label(kind: &str) -> &str {
    match kind {
        "fiction" => "Fiction",
        "nonfiction" => "Nonfiction",
        other => other,
    }
}

#[component]
pub fn Workspace(vault: VaultView, on_open_vault: impl Fn(VaultView) + Copy + Send + Sync + 'static) -> impl IntoView {
    let prefs = expect_context::<Prefs>();
    let editor = StoredValue::new_local(None::<EditorHandle>);
    let spell_bridge = StoredValue::new_local(None::<SpellBridge>);
    let link_bridge = StoredValue::new_local(None::<LinkBridge>);
    let projects = RwSignal::new(vault.projects.clone());
    let project = RwSignal::new(vault.projects.first().map(|p| p.slug.clone()));
    let outline = RwSignal::new(None::<OutlineView>);
    let scene = RwSignal::new(None::<SceneView>);
    // The open note; at most one of `scene` and `note` is set.
    let note = RwSignal::new(None::<NoteView>);
    let notes_list = RwSignal::new(Vec::<NoteView>::new());
    let world = RwSignal::new(None::<(String, String)>);
    let tab = RwSignal::new(Tab::Outline);
    // The open scene's envelope: its header names and hints, fetched again after each save.
    let scene_names = RwSignal::new(None::<SceneNamesView>);
    let names_revision = RwSignal::new(0u32);
    let envelope_open = RwSignal::new(false);
    let search = Search::new();
    let palette_open = RwSignal::new(false);
    let search_focus = RwSignal::new(0u32);
    // Words to highlight in the next scene or note to open, when it's opened from search.
    let pending_terms = StoredValue::new(None::<Vec<String>>);
    let save_state = RwSignal::new(SaveState::Saved);
    let error = RwSignal::new(None::<String>);
    let spell_error = RwSignal::new(None::<String>);
    let words = RwSignal::new(0usize);
    let live_words = RwSignal::<LiveWords>::new(None);
    let markdown = RwSignal::new(String::new());
    let typography = RwSignal::new(vault.typography);
    let show_typography = RwSignal::new(false);
    let show_settings = RwSignal::new(false);
    let show_scene_menu = RwSignal::new(false);
    let show_note_menu = RwSignal::new(false);
    let show_project_panel = RwSignal::new(false);
    let show_project_menu = RwSignal::new(false);
    let panel = RwSignal::new(None::<Panel>);
    // The cut line is showing, waiting for Enter or Escape.
    let cutting = RwSignal::new(false);
    // After a merge: where the merged-in scene starts, in top-level blocks, for the seam.
    let pending_seam = StoredValue::new(None::<u32>);
    let revision = RwSignal::new(0u32);
    let new_project = RwSignal::new(None::<String>);
    let _ = tauri::listen("snapshot-taken", move || {
        revision.try_update(|r| *r += 1);
    });

    let current = move || scene.with_untracked(|s| s.as_ref().map(|s| s.slug.clone()));
    let flush = move || {
        editor.with_value(|h| {
            if let Some(h) = h {
                h.flush();
            }
        })
    };
    let report = move |e: String| error.set(Some(e));

    // The open project's cut bin, and the notice after something goes in.
    let bin_items = RwSignal::new(Vec::<CutView>::new());
    // The piece in the bin that search opened, by owner, whether that's a world, and name.
    let bin_found = RwSignal::new(None::<(String, bool, String)>);
    let notice = RwSignal::new(None::<Notice>);
    let notices = StoredValue::new(0u32);
    // A passage to put back once its scene has opened.
    let pending_passage = StoredValue::new(None::<CutView>);

    let load_bin = move |slug: String| {
        spawn_local(async move {
            match tauri::bin_items(&slug).await {
                Ok(items) => bin_items.set(items),
                Err(e) => report(e),
            }
        });
    };

    let show_notice = move |text: &'static str, undo: Option<CutView>| {
        let id = notices.get_value() + 1;
        notices.set_value(id);
        notice.set(Some(Notice { text, undo, id }));
        set_timeout(
            move || {
                if notice.try_with_untracked(|n| n.as_ref().map(|n| n.id)) == Some(Some(id)) {
                    notice.try_set(None);
                }
            },
            NOTICE,
        );
    };

    // After a scene or note goes into the bin: shows it there, with Undo for the piece `is_it`
    // picks out.
    let binned = move |project: String, is_it: Box<dyn Fn(&CutView) -> bool>| {
        spawn_local(async move {
            match tauri::bin_items(&project).await {
                Ok(items) => {
                    let undo = items.iter().find(|item| is_it(item)).cloned();
                    bin_items.set(items);
                    show_notice("Moved to the cut bin", undo);
                }
                Err(e) => report(e),
            }
        });
    };

    // The latest save, resolved once it's done. Saves land in order (see `save_scene`), so by
    // then every earlier one is done too.
    let last_save = StoredValue::new_local(None::<js_sys::Promise>);
    let on_change = move |md: String, count: u32| {
        words.set(count as usize);
        markdown.set(md.clone());
        let save: tauri::LocalFuture<Result<(), String>> =
            if let Some(key) = note.with_untracked(|n| n.as_ref().map(NoteView::key)) {
                Box::pin(async move { tauri::save_note(&key, &md).await })
            } else {
                let (Some(p), Some(s)) = (project.get_untracked(), current()) else { return };
                live_words.set(Some((s.clone(), count as usize)));
                Box::pin(async move {
                    let saved = tauri::save_scene(&p, &s, &md).await;
                    if saved.is_ok() {
                        names_revision.update(|r| *r += 1);
                    }
                    saved
                })
            };
        save_state.set(SaveState::Saving);
        let save = wasm_bindgen_futures::future_to_promise(async move {
            save_state.set(match save.await {
                Ok(()) => SaveState::Saved,
                Err(e) => SaveState::Failed(e),
            });
            Ok(wasm_bindgen::JsValue::UNDEFINED)
        });
        last_save.set_value(Some(save));
    };

    // Before the window closes: saves what's being typed and waits for it to land.
    BeforeClose::set(move || {
        Box::pin(async move {
            flush();
            // The editor counts a failed save as done, so after one it's tried again.
            if matches!(save_state.get_untracked(), SaveState::Failed(_))
                && let Some((md, count)) = editor.with_value(|h| h.as_ref().map(|h| (h.markdown(), h.word_count())))
            {
                on_change(md, count);
            }
            if let Some(save) = last_save.get_value() {
                let _ = wasm_bindgen_futures::JsFuture::from(save).await;
            }
            match save_state.try_get_untracked() {
                Some(SaveState::Failed(e)) => Err(e),
                _ => Ok(()),
            }
        })
    });

    // Passages the editor cut to the bin, by the editor's id for the move, so Undo can take
    // them out of the bin again.
    let binned_passages = StoredValue::new(HashMap::<u32, CutView>::new());

    // A passage that's back in the open scene leaves the bin, once the scene is saved with it.
    let leave_bin = move |item: CutView| {
        let Some(p) = project.get_untracked() else { return };
        let key = (item.owner.clone(), item.world, item.name.clone());
        let same = move |other: &CutView| (&other.owner, other.world, &other.name) == (&key.0, key.1, &key.2);
        binned_passages.update_value(|binned| binned.retain(|_, other| !same(other)));
        if notice.with_untracked(|n| n.as_ref().and_then(|n| n.undo.as_ref()).is_some_and(same)) {
            notice.set(None);
        }
        flush();
        spawn_local(async move {
            if let Some(save) = last_save.get_value() {
                let _ = wasm_bindgen_futures::JsFuture::from(save).await;
            }
            if matches!(save_state.get_untracked(), SaveState::Failed(_)) {
                return;
            }
            match tauri::remove_from_bin(&item).await {
                Ok(()) => load_bin(p),
                Err(e) => report(e),
            }
        });
    };

    // Puts a passage from the bin back into the open scene: in its old place if that's still
    // there, else at the cursor.
    let put_back = move |item: CutView| {
        let Some(passage) = item.passage.clone() else { return };
        let Some(at_spot) = editor.with_value(|h| h.as_ref().map(|h| h.restore_passage(&passage))) else { return };
        leave_bin(item);
        if !at_spot {
            show_notice("Its old place had changed, so it went in at the cursor", None);
        }
    };

    let close_scene = move || {
        scene.set(None);
        note.set(None);
        editor.with_value(|h| {
            if let Some(h) = h {
                h.set_content("");
                h.set_bin_enabled(false);
            }
        });
        words.set(0);
        markdown.set(String::new());
    };

    // Before another scene or note loads: saves what's open and queues a snapshot of it.
    let leave = move || {
        // Loading new text takes the cut line away with it.
        cutting.set(false);
        // Flushing saves the scene being left while it's still current; the snapshot is
        // queued after that save.
        flush();
        // The scene being left keeps its latest count in the outline.
        if let Some((left, count)) = live_words.get_untracked() {
            live_words.set(None);
            outline.update(|o| {
                if let Some(scene) = o.as_mut().and_then(|o| {
                    o.chapters.iter_mut().flat_map(|c| &mut c.scenes).chain(&mut o.unplaced).find(|x| x.slug == left)
                }) {
                    scene.words = count;
                }
            });
        }
        if current().is_some() || note.with_untracked(Option::is_some) {
            spawn_local(async {
                let _ = tauri::snapshot_now().await;
            });
        }
    };

    let open_scene = move |slug: String| {
        let Some(p) = project.get_untracked() else { return };
        leave();
        spawn_local(async move {
            match tauri::open_scene(&p, &slug).await {
                Ok(opened) => {
                    let seam = pending_seam.try_update_value(Option::take).flatten();
                    let terms = pending_terms.try_update_value(Option::take).flatten();
                    let passage = pending_passage.try_update_value(Option::take).flatten();
                    editor.with_value(|h| {
                        if let Some(h) = h {
                            h.set_content(&opened.markdown);
                            h.set_bin_enabled(true);
                            h.set_highlights(&terms_array(terms.as_deref().unwrap_or_default()), false);
                            words.set(h.word_count() as usize);
                            markdown.set(h.markdown());
                        }
                    });
                    note.set(None);
                    scene.set(Some(opened.scene));
                    // Results opened from the Search tab keep it showing.
                    if tab.get_untracked() != Tab::Search {
                        tab.set(Tab::Outline);
                    }
                    save_state.set(SaveState::Saved);
                    // The piece is hidden while no scene is open (as during a split or merge), and
                    // a hidden editor can't take the cursor, so wait until it shows again.
                    request_animation_frame(move || {
                        editor.with_value(|h| {
                            if let Some(h) = h {
                                h.focus();
                                if let Some(index) = seam {
                                    h.show_seam(index);
                                }
                                if let Some(terms) = &terms {
                                    h.set_highlights(&terms_array(terms), true);
                                }
                            }
                        });
                        if let Some(item) = passage {
                            put_back(item);
                        }
                    });
                }
                Err(e) => report(e),
            }
        });
    };

    let open_note = move |key: NoteKey| {
        leave();
        spawn_local(async move {
            match tauri::open_note(&key).await {
                Ok(opened) => {
                    let terms = pending_terms.try_update_value(Option::take).flatten();
                    editor.with_value(|h| {
                        if let Some(h) = h {
                            h.set_content(&opened.markdown);
                            h.set_bin_enabled(false);
                            h.set_highlights(&terms_array(terms.as_deref().unwrap_or_default()), false);
                            words.set(h.word_count() as usize);
                            markdown.set(h.markdown());
                        }
                    });
                    scene.set(None);
                    note.set(Some(opened.note));
                    if tab.get_untracked() != Tab::Search {
                        tab.set(Tab::Notes);
                    }
                    save_state.set(SaveState::Saved);
                    request_animation_frame(move || {
                        editor.with_value(|h| {
                            if let Some(h) = h {
                                h.focus();
                                if let Some(terms) = &terms {
                                    h.set_highlights(&terms_array(terms), true);
                                }
                            }
                        });
                    });
                }
                Err(e) => report(e),
            }
        });
    };

    let load_notes = move |slug: String| {
        spawn_local(async move {
            match tauri::project_notes(&slug).await {
                Ok(found) => {
                    notes_list.set(found.notes);
                    world.set(found.world);
                }
                Err(e) => report(e),
            }
        });
    };

    // The envelope follows the open scene, its saves, and the notes there are.
    Effect::new(move |_| {
        names_revision.track();
        notes_list.track();
        let slug = scene.with(|s| s.as_ref().map(|s| s.slug.clone()));
        let (Some(p), Some(slug)) = (project.get_untracked(), slug) else {
            scene_names.set(None);
            return;
        };
        let wanted = slug.clone();
        envelope::load(
            async move { tauri::scene_names(&p, &slug).await },
            move || scene.with_untracked(|s| s.as_ref().map(|s| s.slug.as_str()) == Some(wanted.as_str())),
            move |view| scene_names.set(Some(view)),
        );
    });

    let set_scene_names = move |field: String, names: Vec<String>| {
        let (Some(p), Some(s)) = (project.get_untracked(), current()) else { return };
        spawn_local(async move {
            match tauri::set_scene_names(&p, &s, &field, &names).await {
                Ok(view) => scene_names.set(Some(view)),
                Err(e) => report(e),
            }
        });
    };

    // Making a note from a link keeps you where you are; the link stops being basted.
    let make_linked_note = move |title: String, kind: String| {
        let Some(p) = project.get_untracked() else { return };
        spawn_local(async move {
            match tauri::create_note(&p, &kind, &title).await {
                Ok(_) => load_notes(p),
                Err(e) => report(e),
            }
        });
    };

    // Links and spelling follow the notes: called whenever the list changes, and once the
    // bridges exist.
    let note_labels = Memo::new(move |_| {
        outline.with(|o| o.as_ref().map_or(ProjectKind::Fiction, |o| notes::project_kind(&o.project.kind)))
    });
    let share_notes = move || {
        let Some(p) = project.get_untracked() else { return };
        let kind = note_labels.get_untracked();
        let world_slug = world.with_untracked(|w| w.as_ref().map(|(slug, _)| slug.clone()));
        notes_list.with_untracked(|list| {
            link_bridge.with_value(|b| {
                if let Some(b) = b {
                    b.set_notes(&p, world_slug.as_deref(), list, kind);
                }
            });
            spell_bridge.with_value(|b| {
                if let Some(b) = b {
                    b.set_names(name_words(list));
                }
            });
        });
        editor.with_value(|h| {
            if let Some(h) = h {
                h.refresh_links();
                h.recheck_spelling();
            }
        });
    };
    Effect::new(move |_| {
        notes_list.track();
        world.track();
        note_labels.track();
        share_notes();
    });

    // An outline from the backend counts every saved word, so the live count can go.
    let set_outline = move |view: OutlineView| {
        live_words.set(None);
        outline.set(Some(view));
    };

    // After any outline change: refresh the open scene's title and status, and move on if
    // the open scene is gone. The outline is set first, so views of the scene see the new one.
    let apply = move |view: OutlineView| {
        let open = current();
        let info = open.as_deref().and_then(|s| find(&view, s));
        let first = reading_order(&view).first().map(|s| s.slug.clone());
        set_outline(view);
        match info {
            Some(info) => scene.set(Some(info)),
            None if open.is_some() => {
                scene.set(None);
                match first {
                    Some(first) => open_scene(first),
                    None => close_scene(),
                }
            }
            None => {}
        }
    };

    let load_project_at = move |slug: String, target: Option<String>| {
        flush();
        load_notes(slug.clone());
        load_bin(slug.clone());
        spawn_local(async move {
            match tauri::project_outline(&slug).await {
                Ok(view) => {
                    let first = target.or_else(|| reading_order(&view).first().map(|s| s.slug.clone()));
                    scene.set(None);
                    note.set(None);
                    set_outline(view);
                    match first {
                        Some(first) => open_scene(first),
                        None => close_scene(),
                    }
                }
                Err(e) => report(e),
            }
        });
    };

    let load_project = move |slug: String| load_project_at(slug, None);

    // Opens a search result, with its matches highlighted; a scene in another project opens
    // that project first. Something in the cut bin is shown there.
    let open_hit = move |hit: HitView| {
        if hit.kind == "cut" {
            let owner = hit.world.clone().or(hit.project.clone()).unwrap_or_default();
            bin_found.set(Some((owner, hit.world.is_some(), hit.key)));
            if let Some(p) = hit.project.filter(|p| project.get_untracked().as_ref() != Some(p)) {
                project.set(Some(p.clone()));
                load_project_at(p, None);
            }
            panel.set(Some(Panel::Bin));
            return;
        }
        pending_terms.set_value(Some(search.terms()));
        if hit.kind == "scene" {
            let Some(p) = hit.project else { return };
            if project.get_untracked().as_ref() == Some(&p) {
                open_scene(hit.key);
            } else {
                project.set(Some(p.clone()));
                load_project_at(p, Some(hit.key));
            }
        } else {
            let world = hit.world.is_some();
            let owner = hit.world.or(hit.project).unwrap_or_default();
            open_note(NoteKey { owner, world, path: hit.key });
        }
    };

    // Ctrl+K (Cmd+K on a Mac) opens the search box from anywhere.
    let shortcut = window_event_listener(leptos::ev::keydown, move |ev| {
        if (ev.ctrl_key() || ev.meta_key()) && ev.key().eq_ignore_ascii_case("k") {
            ev.prevent_default();
            palette_open.try_set(true);
        }
    });
    on_cleanup(move || shortcut.remove());

    let load_spellchecker = move || {
        spawn_local(async move {
            let speller = tauri::spell_dictionary().await.and_then(|d| {
                let mut speller = Speller::new(&d.aff, &d.dic).map_err(|e| e.to_string())?;
                for word in &d.personal_words {
                    // A malformed line in the word list shouldn't disable spellcheck.
                    let _ = speller.add(word);
                }
                Ok(speller)
            });
            match speller {
                Ok(speller) => {
                    let bridge = SpellBridge::new(speller, move |word| {
                        spawn_local(async move {
                            if let Err(e) = tauri::add_to_dictionary(&word).await {
                                spell_error.set(Some(format!("couldn't save “{word}”: {e}")));
                            }
                        });
                    });
                    editor.with_value(|h| {
                        if let Some(h) = h {
                            h.set_spellchecker(bridge.as_object());
                        }
                    });
                    spell_bridge.set_value(Some(bridge));
                    share_notes();
                }
                Err(e) => spell_error.set(Some(e)),
            }
        });
    };

    let on_ready = move || {
        let bridge = LinkBridge::new(open_note, make_linked_note);
        editor.with_value(|h| {
            if let Some(h) = h {
                h.set_link_resolver(bridge.as_object());
            }
        });
        link_bridge.set_value(Some(bridge));
        share_notes();
        load_spellchecker();
        if let Some(p) = project.get_untracked() {
            load_project(p);
        }
    };

    // --- Scene actions ---------------------------------------------------------------------

    let with_scene = move |action: &dyn Fn(String, String)| {
        if let (Some(p), Some(s)) = (project.get_untracked(), current()) {
            flush();
            action(p, s);
        }
    };

    let rename = move |title: String| {
        with_scene(&move |p, s| {
            let title = title.clone();
            spawn_local(async move {
                match tauri::rename_scene(&p, &s, &title).await {
                    Ok(view) => apply(view),
                    Err(e) => report(e),
                }
            });
        });
    };

    let set_summary = move |summary: String| {
        with_scene(&move |p, s| {
            let summary = summary.clone();
            spawn_local(async move {
                match tauri::set_scene_summary(&p, &s, &summary).await {
                    Ok(view) => apply(view),
                    Err(e) => report(e),
                }
            });
        });
    };

    let set_status = move |status: String| {
        with_scene(&move |p, s| {
            let status = status.clone();
            spawn_local(async move {
                match tauri::set_scene_status(&p, &s, &status).await {
                    Ok(view) => apply(view),
                    Err(e) => report(e),
                }
            });
        });
    };

    let split = move || {
        with_scene(&move |p, s| {
            let (before, after) = editor.with_value(|h| h.as_ref().map(EditorHandle::split_parts)).unwrap_or_default();
            spawn_local(async move {
                match tauri::split_scene(&p, &s, &before, &after).await {
                    Ok(created) => {
                        // The editor still holds the whole text; don't save it over the split.
                        scene.set(None);
                        set_outline(created.outline);
                        open_scene(created.scene);
                    }
                    Err(e) => report(e),
                }
            });
        });
    };

    // Split shows the cut line first; Enter (or "Cut here") splits, Escape puts it away.
    let show_cut_line = move |on: bool| {
        cutting.set(on);
        editor.with_value(|h| {
            if let Some(h) = h {
                h.show_cut_line(on);
            }
        });
    };
    let on_cut_confirm = move || {
        show_cut_line(false);
        split();
    };
    let on_cut_cancel = move || show_cut_line(false);

    let merge = move || {
        with_scene(&move |p, s| {
            // The merged-in scene's text goes after this one's, so the seam goes after its blocks.
            pending_seam.set_value(editor.with_value(|h| h.as_ref().map(EditorHandle::block_count)));
            spawn_local(async move {
                match tauri::merge_scene(&p, &s).await {
                    Ok(view) => {
                        apply(view);
                        scene.set(None);
                        load_bin(p);
                        open_scene(s);
                    }
                    Err(e) => {
                        pending_seam.set_value(None);
                        report(e);
                    }
                }
            });
        });
    };

    let cut = move || {
        with_scene(&move |p, s| {
            let next = outline.with_untracked(|o| o.as_ref().and_then(|o| neighbour(o, &s)));
            spawn_local(async move {
                match tauri::cut_scene(&p, &s).await {
                    Ok(view) => {
                        binned(p, Box::new(move |item| item.kind == "scene" && item.scene.as_deref() == Some(s.as_str())));
                        scene.set(None);
                        set_outline(view);
                        match next {
                            Some(next) => open_scene(next),
                            None => close_scene(),
                        }
                    }
                    Err(e) => report(e),
                }
            });
        });
    };

    let on_restore = move |md: String| {
        editor.with_value(|h| {
            if let Some(h) = h {
                h.set_content(&md);
                words.set(h.word_count() as usize);
            }
        });
        markdown.set(md);
        save_state.set(SaveState::Saved);
    };

    // --- The cut bin -----------------------------------------------------------------------

    // The editor has taken the passage out of the scene (by a cut, or by undoing a restore);
    // it goes into the bin, or back if it can't.
    let cut_to_bin = move |passage: Passage, id: u32| {
        let (Some(p), Some(s)) = (project.get_untracked(), current()) else { return };
        spawn_local(async move {
            match tauri::cut_passage(&p, &s, &passage).await {
                Ok(item) => {
                    binned_passages.update_value(|binned| _ = binned.insert(id, item.clone()));
                    load_bin(p);
                    show_notice("Moved to the cut bin", Some(item));
                }
                Err(e) if current().as_ref() == Some(&s) => {
                    editor.with_value(|h| {
                        if let Some(h) = h {
                            h.restore_passage(&passage);
                        }
                    });
                    report(format!("Couldn't put it in the cut bin, so it's back in the scene: {e}"));
                }
                Err(e) => report(format!("Couldn't put this in the cut bin ({e}): {}", passage.markdown.trim())),
            }
        });
    };

    // Undo (or Redo) has put a binned passage back in the text.
    let back_from_bin = move |id: u32| {
        if let Some(item) = binned_passages.try_update_value(|binned| binned.remove(&id)).flatten() {
            leave_bin(item);
        }
    };

    // A passage goes back into its own scene, opened if need be, when its old place is still
    // there. Otherwise it goes in at the cursor in the open scene.
    let restore_passage = move |item: CutView| {
        let (Some(p), Some(passage)) = (project.get_untracked(), item.passage.clone()) else { return };
        let open = current();
        let source = item.scene.clone().filter(|s| outline.with_untracked(|o| o.as_ref().is_some_and(|o| find(o, s).is_some())));
        match source {
            Some(source) if open.as_ref() == Some(&source) => put_back(item),
            Some(source) => spawn_local(async move {
                let found = tauri::open_scene(&p, &source).await.is_ok_and(|o| editor::has_spot(&o.markdown, &passage));
                if found || open.is_none() {
                    pending_passage.set_value(Some(item));
                    open_scene(source);
                } else {
                    put_back(item);
                }
            }),
            None if open.is_some() => put_back(item),
            None => report("Its scene is gone. Open a scene and put the cursor where it should go.".to_owned()),
        }
    };

    let restore_from_bin = move |item: CutView| {
        let Some(p) = project.get_untracked() else { return };
        notice.set(None);
        match item.kind.as_str() {
            "passage" => restore_passage(item),
            "note" => spawn_local(async move {
                match tauri::restore_note(&item).await {
                    Ok(restored) => {
                        load_bin(p.clone());
                        load_notes(p);
                        open_note(restored.key());
                    }
                    Err(e) => report(e),
                }
            }),
            _ => spawn_local(async move {
                flush();
                match tauri::restore_scene(&p, &item.name).await {
                    Ok(created) => {
                        load_bin(p);
                        apply(created.outline);
                        open_scene(created.scene);
                    }
                    Err(e) => report(e),
                }
            }),
        }
    };

    // --- Note actions ----------------------------------------------------------------------

    let create_note = move |kind: String, title: String| {
        let Some(p) = project.get_untracked() else { return };
        spawn_local(async move {
            match tauri::create_note(&p, &kind, &title).await {
                Ok(created) => {
                    load_notes(p);
                    open_note(created.key());
                }
                Err(e) => report(e),
            }
        });
    };

    // Renaming rewrites links in other files, and maybe in this one, so the note is saved
    // first and its text reloaded after.
    let rename_note = move |title: String| {
        let (Some(p), Some(key)) = (project.get_untracked(), note.with_untracked(|n| n.as_ref().map(NoteView::key))) else {
            return;
        };
        flush();
        spawn_local(async move {
            match tauri::rename_note(&key, &title).await {
                Ok(renamed) => {
                    note.set(Some(renamed));
                    load_notes(p);
                    if let Ok(opened) = tauri::open_note(&key).await {
                        let unchanged = editor.with_value(|h| h.as_ref().is_some_and(|h| h.markdown() == opened.markdown));
                        if !unchanged && note.with_untracked(|n| n.as_ref().map(NoteView::key)) == Some(key) {
                            editor.with_value(|h| {
                                if let Some(h) = h {
                                    h.set_content(&opened.markdown);
                                }
                            });
                        }
                    }
                }
                Err(e) => {
                    // Put the old title back in the field.
                    note.update(|_| {});
                    report(e);
                }
            }
        });
    };

    let set_aliases = move |text: String| {
        let (Some(p), Some(key)) = (project.get_untracked(), note.with_untracked(|n| n.as_ref().map(NoteView::key))) else {
            return;
        };
        let aliases: Vec<String> = text.split(',').map(|a| a.trim().to_owned()).filter(|a| !a.is_empty()).collect();
        spawn_local(async move {
            match tauri::set_note_aliases(&key, &aliases).await {
                Ok(updated) => {
                    note.set(Some(updated));
                    load_notes(p);
                }
                Err(e) => report(e),
            }
        });
    };

    // --- Projects and vaults ---------------------------------------------------------------

    let choose_project = move |slug: String| {
        project.set(Some(slug.clone()));
        load_project(slug);
    };

    let create_project = move |title: String, kind: String| {
        new_project.set(None);
        spawn_local(async move {
            match tauri::create_project(&title, &kind).await {
                Ok(created) => {
                    let slug = created.slug.clone();
                    projects.update(|list| {
                        list.push(created);
                        list.sort_by_key(|p: &ProjectView| p.title.to_lowercase());
                    });
                    choose_project(slug);
                }
                Err(e) => report(e),
            }
        });
    };

    let project_saved = move || {
        spawn_local(async move {
            if let Ok(Some(vault)) = tauri::current_vault().await {
                projects.set(vault.projects);
            }
        });
        if let Some(p) = project.get_untracked() {
            load_project(p);
        }
    };

    let promote = move || {
        let (Some(p), Some(n)) = (project.get_untracked(), note.get_untracked()) else { return };
        flush();
        spawn_local(async move {
            match tauri::promote_note(&p, &n.path).await {
                Ok(moved) => {
                    load_notes(p);
                    // The editor has the note's latest text, already saved; reopen it where it is now.
                    note.set(None);
                    open_note(moved.key());
                }
                Err(e) => report(e),
            }
        });
    };

    // Like cutting a scene: the next note in the list opens in its place.
    let cut_note = move || {
        let (Some(p), Some(n)) = (project.get_untracked(), note.get_untracked()) else { return };
        let key = n.key();
        let next = notes_list.with_untracked(|list| {
            let i = list.iter().position(|x| x.key() == key)?;
            list.get(i + 1).or_else(|| i.checked_sub(1).and_then(|j| list.get(j))).map(NoteView::key)
        });
        flush();
        spawn_local(async move {
            match tauri::cut_note(&key).await {
                Ok(()) => {
                    let title = n.title.clone();
                    binned(
                        p.clone(),
                        Box::new(move |item| item.kind == "note" && item.title == title && item.owner == key.owner && item.world == key.world),
                    );
                    note.set(None);
                    load_notes(p);
                    match next {
                        Some(next) => open_note(next),
                        None => close_scene(),
                    }
                }
                Err(e) => report(e),
            }
        });
    };

    let open_other_vault = move |_| {
        flush();
        spawn_local(async move {
            match tauri::open_vault().await {
                Ok(Some(vault)) => on_open_vault(vault),
                Ok(None) => {}
                Err(e) => report(e),
            }
        });
    };

    // --- What the views show ---------------------------------------------------------------

    let toggle = move |which: Panel| panel.update(|p| *p = if *p == Some(which) { None } else { Some(which) });
    // A memo, so retitling the note or changing the scene's status doesn't reload the history.
    let target = Memo::new(move |_| match note.get() {
        Some(n) => Some(HistoryTarget::Note(n.key())),
        None => project.get().zip(scene.get()).map(|(project, s)| HistoryTarget::Scene { project, scene: s.slug }),
    });
    let current_slug = Signal::derive(move || scene.get().map(|s| s.slug));
    let has_scene = move || scene.with(Option::is_some);
    let has_note = move || note.with(Option::is_some);
    let scene_title = Signal::derive(move || scene.with(|s| s.as_ref().map(|s| s.title.clone()).unwrap_or_default()));
    let scene_summary = Signal::derive(move || scene.with(|s| s.as_ref().map(|s| s.summary.clone()).unwrap_or_default()));
    let has_doc = move || has_scene() || has_note();
    let project_kind = Signal::derive(move || {
        outline.with(|o| o.as_ref().map_or(ProjectKind::Fiction, |o| notes::project_kind(&o.project.kind)))
    });
    let statuses = Signal::derive(move || outline.with(|o| o.as_ref().map(|o| o.statuses.clone()).unwrap_or_default()));
    let current_note = Signal::derive(move || note.with(|n| n.as_ref().map(NoteView::key)));
    let titles_of = move |kind: &'static str| {
        Signal::derive(move || notes_list.with(|all| all.iter().filter(|n| n.kind == kind).map(|n| n.title.clone()).collect::<Vec<_>>()))
    };
    let characters = titles_of("character");
    let threads = titles_of("thread");
    search.watch(project.into(), notes_list.into());

    // Under a result's title: a scene's chapter (or its project, if it's another one), a
    // note's type.
    let hit_subtitle = move |hit: &HitView| -> String {
        if hit.kind == "cut" {
            return match hit.note_type.as_deref() {
                Some("passage") => "Passage".to_owned(),
                Some("scene") | None => "Scene".to_owned(),
                Some(kind) => notes::kind_label(kind, project_kind.get_untracked()).to_owned(),
            };
        }
        if hit.kind != "scene" {
            return notes::kind_label(hit.note_type.as_deref().unwrap_or("note"), project_kind.get_untracked()).to_owned();
        }
        if hit.project != project.get_untracked() {
            return projects.with_untracked(|list| {
                list.iter().find(|p| Some(&p.slug) == hit.project.as_ref()).map(|p| p.title.clone()).unwrap_or_default()
            });
        }
        outline.with_untracked(|o| o.as_ref().and_then(|o| place(o, &hit.key)).and_then(|p| p.chapter)).unwrap_or_default()
    };
    let hit_is_open = move |hit: &HitView| {
        if hit.kind == "cut" {
            panel.get() == Some(Panel::Bin)
                && bin_found.with(|f| f.as_ref().is_some_and(|(owner, world, name)| *name == hit.key && *world == hit.world.is_some() && Some(owner) == hit.world.as_ref().or(hit.project.as_ref())))
        } else if hit.kind == "scene" {
            hit.project == project.get() && scene.with(|s| s.as_ref().is_some_and(|s| s.slug == hit.key))
        } else {
            note.with(|n| {
                n.as_ref().is_some_and(|n| n.path == hit.key && n.world == hit.world.is_some() && Some(&n.owner) == hit.world.as_ref().or(hit.project.as_ref()))
            })
        }
    };
    let current_place = move || {
        let slug = scene.with(|s| s.as_ref().map(|s| s.slug.clone()))?;
        outline.with(|o| o.as_ref().and_then(|o| place(o, &slug)))
    };

    let piece_number = Signal::derive(move || current_place().map(|p| format!("Piece {}", p.number)).unwrap_or_default());

    let project_title = move || {
        let slug = project.get();
        projects.with(|list| list.iter().find(|p| Some(&p.slug) == slug.as_ref()).map(|p| p.title.clone()))
    };
    let project_meta = move || {
        outline.with(|o| {
            o.as_ref().map(|o| {
                let total = live_words.with(|live| outline::words(o.chapters.iter().flat_map(|c| &c.scenes), live));
                format!("{} · {}", kind_label(&o.project.kind), outline::format_words(total))
            })
        })
    };
    let vault_name = vault.path.rsplit(['/', '\\']).find(|s| !s.is_empty()).unwrap_or(&vault.path).to_owned();
    let vault_title = format!("{}\nOpen another vault…", vault.path);

    let note_breadcrumb = move || {
        note.with(|n| {
            n.as_ref().map(|n| {
                let owner = match (&n.world, world.get()) {
                    (true, Some((_, name))) => name,
                    _ => "Notes".to_owned(),
                };
                let kind = NoteKind::parse(&n.kind).unwrap_or(NoteKind::Note).plural(project_kind.get());
                view! {
                    <span>{owner}</span>
                    <Icon glyph=Glyph::ChevronRight size=12 />
                    <span>{kind}</span>
                }
            })
        })
    };

    let breadcrumb = move || {
        if has_note() {
            return note_breadcrumb().map(IntoAny::into_any);
        }
        current_place().map(|place| match place.chapter {
            None => view! { <span>"Not in the outline"</span> }.into_any(),
            Some(chapter) => view! {
                {place.part.map(|part| view! {
                    <span>{part}</span>
                    <Icon glyph=Glyph::ChevronRight size=12 />
                })}
                <span>{chapter}</span>
            }
            .into_any(),
        })
    };

    let piece_label = move || {
        current_place().map(|place| match place.chapter {
            Some(chapter) => format!("Piece {} · {chapter}", place.number),
            None => format!("Piece {} · not in the outline", place.number),
        })
    };

    // Rebuilt only when the scene changes, so typing in the title isn't interrupted.
    let piece_head = move || {
        scene.get().map(|s| {
            let statuses = outline.with_untracked(|o| o.as_ref().map(|o| o.statuses.clone()).unwrap_or_default());
            let progress = status::progress(&s.status, &statuses);
            view! {
                <header class="piece-head">
                    <div class="piece-label">{piece_label}</div>
                    <input
                        class="scene-name"
                        aria-label="Scene title"
                        prop:value=s.title.clone()
                        on:change=move |ev| rename(event_target_value(&ev))
                    />
                    <div class="piece-meta">
                        <label class="status-pill" title="Status">
                            <StatusMark progress=progress />
                            <span>{s.status.clone()}</span>
                            <Icon glyph=Glyph::ChevronDown size=12 />
                            <select aria-label="Status" prop:value=s.status.clone() on:change=move |ev| set_status(event_target_value(&ev))>
                                {statuses.into_iter().map(|st| view! { <option value=st.clone()>{st.clone()}</option> }).collect_view()}
                            </select>
                        </label>
                        <span>{move || outline::format_words(words.get())}</span>
                    </div>
                </header>
            }
        })
    };

    // Rebuilt only when the note changes, like the scene's head.
    let note_head = move || {
        note.get().map(|n| {
            let label = notes::kind_label(&n.kind, project_kind.get_untracked());
            let owner = n.world.then(|| world.get_untracked().map(|(_, name)| format!("from {name}"))).flatten();
            view! {
                <header class="piece-head note-head">
                    <div class="piece-label">
                        <span>{label}</span>
                        {owner.map(|o| view! { <span class="muted">{o}</span> })}
                    </div>
                    <input
                        class="scene-name"
                        aria-label="Note title"
                        prop:value=n.title.clone()
                        on:change=move |ev| rename_note(event_target_value(&ev))
                    />
                    <label class="aliases">
                        <span class="aliases-label">"Also"</span>
                        <input
                            class="aliases-input"
                            aria-label="Other names"
                            placeholder="other names, separated by commas"
                            prop:value=n.aliases.join(", ")
                            on:change=move |ev| set_aliases(event_target_value(&ev))
                        />
                    </label>
                </header>
            }
        })
    };

    view! {
        <div class="workspace-grid">
            <aside class="sidebar">
                <div class="sidebar-head">
                    <span class="spool">
                        <Icon glyph=Glyph::Spool size=24 />
                    </span>
                    <div class="project">
                        <div class="project-row">
                            <div class="project-picker">
                                <span class="project-title">{project_title}</span>
                                <Icon glyph=Glyph::ChevronDown size=14 />
                                <select
                                    aria-label="Project"
                                    prop:value=move || project.get().unwrap_or_default()
                                    on:change=move |ev| choose_project(event_target_value(&ev))
                                >
                                    <For each=move || projects.get() key=|p| p.slug.clone() let(p)>
                                        <option value=p.slug.clone()>{p.title.clone()}</option>
                                    </For>
                                </select>
                            </div>
                            <div class="popover-anchor">
                                <button
                                    class="icon-button"
                                    title="Project actions"
                                    aria-label="Project actions"
                                    class:active=move || show_project_menu.get() || show_project_panel.get()
                                    on:click=move |_| show_project_menu.update(|v| *v = !*v)
                                >
                                    <Icon glyph=Glyph::More />
                                </button>
                                <Show when=move || show_project_menu.get()>
                                    <div class="backdrop" on:click=move |_| show_project_menu.set(false)></div>
                                    <div class="menu" role="menu" aria-label="Project actions">
                                        <button role="menuitem" on:click=move |_| {
                                            show_project_menu.set(false);
                                            show_project_panel.set(true);
                                        }>
                                            <Icon glyph=Glyph::Settings />
                                            "Project settings…"
                                        </button>
                                        <button role="menuitem" on:click=move |_| {
                                            show_project_menu.set(false);
                                            new_project.set(Some(String::new()));
                                        }>
                                            <Icon glyph=Glyph::Plus />
                                            "New project…"
                                        </button>
                                    </div>
                                </Show>
                                <Show when=move || show_project_panel.get() && project.get().is_some()>
                                    <div class="backdrop" on:click=move |_| show_project_panel.set(false)></div>
                                    <ProjectPanel
                                        project=project.get_untracked().unwrap_or_default()
                                        on_saved=project_saved
                                        on_close=move || show_project_panel.set(false)
                                    />
                                </Show>
                            </div>
                        </div>
                        <span class="project-meta">{project_meta}</span>
                    </div>
                </div>
                <Show when=move || new_project.get().is_some()>
                    <NewProjectForm on_create=create_project on_cancel=move || new_project.set(None) />
                </Show>
                <div class="tabs" role="tablist" aria-label="Sidebar">
                    <button
                        role="tab"
                        class="tab"
                        aria-selected=move || (tab.get() == Tab::Outline).to_string()
                        on:click=move |_| tab.set(Tab::Outline)
                    >
                        "Outline"
                    </button>
                    <button
                        role="tab"
                        class="tab"
                        aria-selected=move || (tab.get() == Tab::Notes).to_string()
                        on:click=move |_| tab.set(Tab::Notes)
                    >
                        "Notes"
                    </button>
                    <button
                        role="tab"
                        class="tab"
                        aria-selected=move || (tab.get() == Tab::Search).to_string()
                        on:click=move |_| {
                            tab.set(Tab::Search);
                            search_focus.update(|f| *f += 1);
                        }
                    >
                        "Search"
                    </button>
                </div>
                <div class="cut-rule" aria-hidden="true">
                    <Icon glyph=Glyph::Scissors size=14 />
                </div>
                // Both stay mounted, so switching keeps each one's scroll and state.
                <div class="tab-panel" role="tabpanel" hidden=move || tab.get() != Tab::Outline>
                    <OutlineTree
                        project=project
                        outline=outline
                        current=current_slug
                        cutting=cutting
                        live_words=live_words
                        on_open=open_scene
                        on_outline=apply
                        on_error=report
                    />
                </div>
                <div class="tab-panel" role="tabpanel" hidden=move || tab.get() != Tab::Notes>
                    <NotesList
                        notes=notes_list
                        project=project
                        world=world
                        kind=project_kind
                        current=current_note
                        on_open=open_note
                        on_create=create_note
                    />
                </div>
                <div class="tab-panel" role="tabpanel" hidden=move || tab.get() != Tab::Search>
                    <SearchSidebar
                        search=search
                        focus=search_focus
                        statuses=statuses
                        characters=characters
                        threads=threads
                        subtitle=hit_subtitle
                        is_current=hit_is_open
                        on_open=open_hit
                    />
                </div>
                <div class="cut-rule" aria-hidden="true">
                    <Icon glyph=Glyph::Scissors size=14 />
                </div>
                <div class="sidebar-foot">
                    <button class="quiet vault-button" title=vault_title on:click=open_other_vault>
                        <Icon glyph=Glyph::Folder size=15 />
                        <span>{vault_name}</span>
                    </button>
                    <button
                        class="icon-button bin-button"
                        title="Cut bin"
                        aria-label="Cut bin"
                        class:active=move || panel.get() == Some(Panel::Bin)
                        on:click=move |_| toggle(Panel::Bin)
                    >
                        <Icon glyph=Glyph::Basket />
                        <Show when=move || !bin_items.with(Vec::is_empty)>
                            <span class="bin-count">{move || bin_items.with(Vec::len)}</span>
                        </Show>
                    </button>
                    <div class="popover-anchor">
                        <button
                            class="icon-button"
                            title="Settings"
                            aria-label="Settings"
                            class:active=move || show_settings.get()
                            on:click=move |_| show_settings.update(|v| *v = !*v)
                        >
                            <Icon glyph=Glyph::Settings />
                        </button>
                        <Show when=move || show_settings.get()>
                            <div class="backdrop" on:click=move |_| show_settings.set(false)></div>
                            <SettingsPanel />
                        </Show>
                    </div>
                </div>
            </aside>

            <section class="main">
                <header class="topbar">
                    <nav class="breadcrumb" aria-label="Where this scene is">{breadcrumb}</nav>
                    <Show when=has_doc>
                        <span class="save-state" class:error=move || matches!(save_state.get(), SaveState::Failed(_))>
                            {move || match save_state.get() {
                                SaveState::Saved => view! { <Icon glyph=Glyph::Check size=14 /> "Saved" }.into_any(),
                                SaveState::Saving => "Saving…".into_any(),
                                SaveState::Failed(e) => format!("Couldn't save: {e}").into_any(),
                            }}
                        </span>
                        <div class="popover-anchor">
                            <button
                                class="icon-button typography-button"
                                title="Typography"
                                aria-label="Typography"
                                class:active=move || show_typography.get()
                                on:click=move |_| show_typography.update(|v| *v = !*v)
                            >
                                "Aa"
                            </button>
                            <Show when=move || show_typography.get()>
                                <div class="backdrop" on:click=move |_| show_typography.set(false)></div>
                                <TypographySettings typography=typography />
                            </Show>
                        </div>
                        <button
                            class="icon-button"
                            title="History"
                            aria-label="History"
                            class:active=move || panel.get() == Some(Panel::History)
                            on:click=move |_| toggle(Panel::History)
                        >
                            <Icon glyph=Glyph::History size=17 />
                        </button>
                    </Show>
                    <Show when=has_scene>
                        <div class="popover-anchor">
                            <button
                                class="icon-button"
                                title="Scene actions"
                                aria-label="Scene actions"
                                class:active=move || show_scene_menu.get()
                                on:click=move |_| show_scene_menu.update(|v| *v = !*v)
                            >
                                <Icon glyph=Glyph::More size=17 />
                            </button>
                            <Show when=move || show_scene_menu.get()>
                                <div class="backdrop" on:click=move |_| show_scene_menu.set(false)></div>
                                <div class="menu" role="menu" aria-label="Scene actions">
                                    <button role="menuitem" title="Shows a cut line at the cursor; Enter cuts there" on:click=move |_| {
                                        show_scene_menu.set(false);
                                        show_cut_line(true);
                                    }>
                                        <Icon glyph=Glyph::Scissors />
                                        "Split at the cursor"
                                    </button>
                                    <button role="menuitem" title="Add the next scene in this chapter to the end of this one" on:click=move |_| {
                                        show_scene_menu.set(false);
                                        merge();
                                    }>
                                        <Icon glyph=Glyph::Needle />
                                        "Merge with the next scene"
                                    </button>
                                    <hr />
                                    <button role="menuitem" on:click=move |_| {
                                        show_scene_menu.set(false);
                                        cut();
                                    }>
                                        <Icon glyph=Glyph::Basket />
                                        "Move to the cut bin"
                                    </button>
                                    <Show when=move || prefs.markdown_panel.get()>
                                        <hr />
                                        <button role="menuitem" on:click=move |_| {
                                            show_scene_menu.set(false);
                                            toggle(Panel::Markdown);
                                        }>
                                            {move || if panel.get() == Some(Panel::Markdown) { "Hide Markdown" } else { "Show Markdown" }}
                                        </button>
                                    </Show>
                                </div>
                            </Show>
                        </div>
                    </Show>
                    <Show when=has_note>
                        <div class="popover-anchor">
                            <button
                                class="icon-button"
                                title="Note actions"
                                aria-label="Note actions"
                                class:active=move || show_note_menu.get()
                                on:click=move |_| show_note_menu.update(|v| *v = !*v)
                            >
                                <Icon glyph=Glyph::More size=17 />
                            </button>
                            <Show when=move || show_note_menu.get()>
                                <div class="backdrop" on:click=move |_| show_note_menu.set(false)></div>
                                <div class="menu" role="menu" aria-label="Note actions">
                                    {move || {
                                        let in_world = note.with(|n| n.as_ref().is_some_and(|n| n.world));
                                        let world_name = world.get().map(|(_, name)| name);
                                        let (enabled, detail) = match (in_world, world_name) {
                                            (true, Some(name)) => (false, format!("Already in {name}")),
                                            (false, Some(name)) => (true, format!("Moves it to {name}, for every project there")),
                                            (_, None) => (false, "Put the project in a world first (⋯ by its title)".to_owned()),
                                        };
                                        view! {
                                            <button
                                                role="menuitem"
                                                class="with-note"
                                                disabled=!enabled
                                                on:click=move |_| {
                                                    show_note_menu.set(false);
                                                    promote();
                                                }
                                            >
                                                <span>"Promote to world"</span>
                                                <span class="menu-note muted">{detail}</span>
                                            </button>
                                        }
                                    }}
                                    <hr />
                                    <button role="menuitem" on:click=move |_| {
                                        show_note_menu.set(false);
                                        cut_note();
                                    }>
                                        <Icon glyph=Glyph::Basket />
                                        "Move to the cut bin"
                                    </button>
                                    <Show when=move || prefs.markdown_panel.get()>
                                        <hr />
                                        <button role="menuitem" on:click=move |_| {
                                            show_note_menu.set(false);
                                            toggle(Panel::Markdown);
                                        }>
                                            {move || if panel.get() == Some(Panel::Markdown) { "Hide Markdown" } else { "Show Markdown" }}
                                        </button>
                                    </Show>
                                </div>
                            </Show>
                        </div>
                    </Show>
                </header>
                {move || error.get().map(|e| view! {
                    <div class="banner error">
                        <span>{e}</span>
                        <button class="quiet" on:click=move |_| error.set(None)>"Dismiss"</button>
                    </div>
                })}
                {move || spell_error.get().map(|e| view! {
                    <div class="banner error">
                        <span>{format!("Spellcheck unavailable: {e}")}</span>
                    </div>
                })}
                <div class="page" class:empty=move || !has_doc()>
                    // A note's links swatch sits beside it when there's room, else below.
                    <div class="bench">
                        <div class="piece-holder" class:note-holder=has_note>
                            // The editor stays mounted while nothing is open; the piece is only
                            // hidden. A scene draws it as a pattern piece, a note as a swatch.
                            <article class="piece" class:pattern-piece=has_scene class:swatch=has_note>
                                <Show when=has_scene>
                                    <Notches />
                                    <Grainline />
                                </Show>
                                {piece_head}
                                {note_head}
                                <Editor
                                    handle=editor
                                    typography=typography
                                    on_change=on_change
                                    on_cut_confirm=on_cut_confirm
                                    on_cut_cancel=on_cut_cancel
                                    on_cut_to_bin=cut_to_bin
                                    on_back_from_bin=back_from_bin
                                    on_ready=on_ready
                                />
                            </article>
                            <Show when=has_note>
                                <Pin />
                            </Show>
                        </div>
                        <Show when=has_scene>
                            <aside class="envelope-holder" aria-label="Scene details">
                                <EnvelopeCard
                                    names=scene_names
                                    title=scene_title
                                    summary=scene_summary
                                    piece=piece_number
                                    notes=notes_list
                                    kind=project_kind
                                    place="beside"
                                    on_set=set_scene_names
                                    on_summary=set_summary
                                    on_open_note=open_note
                                    on_make_note=make_linked_note
                                />
                            </aside>
                        </Show>
                        <Show when=has_note>
                            <LinksSwatch
                                project=project
                                note=note
                                statuses=statuses
                                on_open_scene=open_scene
                                on_open_note=open_note
                            />
                        </Show>
                    </div>
                    <Show when=move || !has_doc()>
                        <p class="empty-note">"No scene open. Pick one in the outline, or add one with the + on a chapter."</p>
                    </Show>
                </div>
                <Show when=has_scene>
                    // Clicking anywhere else closes the drawer; the tab and the drawer sit above this.
                    <Show when=move || envelope_open.get()>
                        <div class="backdrop envelope-backdrop" on:click=move |_| envelope_open.set(false)></div>
                    </Show>
                    // A drawer on the page's right edge with the tab as its handle: closed, only the
                    // tab shows; open, the envelope slides out with the tab riding along.
                    <div class="envelope-drawer" class:open=move || envelope_open.get()>
                        <button
                            class="envelope-tab"
                            aria-label="Scene details"
                            aria-expanded=move || envelope_open.get().to_string()
                            on:click=move |_| envelope_open.update(|o| *o = !*o)
                        >
                            <Icon glyph=Glyph::Mail size=16 />
                            <span class="envelope-tab-label">"Notions"</span>
                            {move || {
                                let hints = scene_names.with(envelope::hint_count);
                                (hints > 0).then(|| view! {
                                    <span class="envelope-tab-count" title="Named in the text but not listed">{format!("+{hints}")}</span>
                                })
                            }}
                        </button>
                        <div class="envelope-over" inert=move || !envelope_open.get()>
                            <EnvelopeCard
                                names=scene_names
                                title=scene_title
                                summary=scene_summary
                                piece=piece_number
                                notes=notes_list
                                kind=project_kind
                                place="over"
                                on_set=set_scene_names
                                on_summary=set_summary
                                on_open_note=move |key| {
                                    envelope_open.set(false);
                                    open_note(key);
                                }
                                on_make_note=make_linked_note
                                on_close=move |_| envelope_open.set(false)
                            />
                        </div>
                    </div>
                </Show>
                {move || notice.get().map(|n| {
                    view! {
                        <div class="notice" role="status">
                            <Icon glyph=Glyph::Basket size=15 />
                            <span>{n.text}</span>
                            {n.undo.map(|item| view! {
                                <button class="small" on:click=move |_| restore_from_bin(item.clone())>"Undo"</button>
                                <button class="small quiet" on:click=move |_| {
                                    notice.set(None);
                                    panel.set(Some(Panel::Bin));
                                }>
                                    "Open the bin"
                                </button>
                            })}
                        </div>
                    }
                })}
            </section>

            <Show when=move || palette_open.get()>
                <SearchPalette
                    search=search
                    open=palette_open
                    statuses=statuses
                    characters=characters
                    threads=threads
                    subtitle=hit_subtitle
                    on_open=open_hit
                    on_list=move || {
                        tab.set(Tab::Search);
                        search_focus.update(|f| *f += 1);
                    }
                />
            </Show>

            {move || match panel.get() {
                Some(Panel::History) => Some(view! {
                    <HistoryPanel target=target text=markdown revision=revision editor=editor on_restore=on_restore on_close=move || panel.set(None) />
                }.into_any()),
                Some(Panel::Bin) => Some(view! {
                    <BinPanel
                        items=bin_items
                        found=bin_found
                        kind=project_kind
                        scene_title=move |slug: &str| outline.with(|o| o.as_ref().and_then(|o| find(o, slug)).map(|s| s.title))
                        on_restore=restore_from_bin
                        on_close=move || panel.set(None)
                    />
                }.into_any()),
                Some(Panel::Markdown) if prefs.markdown_panel.get() => Some(view! {
                    <aside class="side-panel markdown-panel">
                        <div class="panel-head">
                            <h2>"Markdown"</h2>
                            <button class="icon-button" title="Close" aria-label="Close Markdown" on:click=move |_| panel.set(None)>
                                <Icon glyph=Glyph::Close />
                            </button>
                        </div>
                        <pre>{move || markdown.get()}</pre>
                    </aside>
                }.into_any()),
                _ => None,
            }}
        </div>
    }
}

#[component]
fn NewProjectForm(
    on_create: impl Fn(String, String) + Copy + Send + Sync + 'static,
    on_cancel: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let title = RwSignal::new(String::new());
    let kind = RwSignal::new("fiction".to_owned());
    view! {
        <form class="new-project" on:submit=move |ev| {
            ev.prevent_default();
            on_create(title.get_untracked(), kind.get_untracked());
        }>
            <input
                placeholder="Project title"
                prop:value=move || title.get()
                on:input=move |ev| title.set(event_target_value(&ev))
            />
            <select on:change=move |ev| kind.set(event_target_value(&ev))>
                <option value="fiction">"Fiction"</option>
                <option value="nonfiction">"Nonfiction"</option>
            </select>
            <div class="actions">
                <button type="submit" class="primary">"Create"</button>
                <button type="button" class="quiet" on:click=move |_| on_cancel()>"Cancel"</button>
            </div>
        </form>
    }
}
