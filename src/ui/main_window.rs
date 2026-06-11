use crate::config::AppConfig;
use crate::inbox::{InboxDb, NoteSession};
use crate::known_workspaces::{KnownWorkspace, KnownWorkspaceRegistry};
use crate::launch::LaunchConfig;
use crate::paths::AppPaths;
use crate::ui::compare_shell::CompareShell;
use crate::ui::desk_shell::DeskShell;
use crate::ui::external_file_shell::ExternalFileShell;
use crate::ui::room_map_shell::RoomMapShell;
use crate::ui::search_shell::{SearchNoteTarget, SearchShell};
use crate::ui::tab_bar::{NoteSource, TabBar, TabModel};
use crate::ui::water_workspace_shell::WaterWorkspaceShell;
use crate::ui::workspace_shell::WorkspaceShell;
use crate::ui::{
    absorb_dialog, command_palette, editor_shell, external_file_shell, merge_dialog, modal_host,
    place_note_dialog, version_history_shell,
};
use crate::workspace::{now_iso8601, WorkspaceDb, WorkspaceNoteSession};
use gio::prelude::*;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow};
use std::cell::RefCell;
use std::rc::Rc;

pub struct MainWindow;

impl MainWindow {
    pub fn new(
        app: &Application,
        launch: &LaunchConfig,
        config: Rc<AppConfig>,
        paths: Rc<AppPaths>,
        db: Rc<RefCell<Option<InboxDb>>>,
    ) -> ApplicationWindow {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("Blot")
            .default_width(1100)
            .default_height(740)
            .build();
        window.add_css_class("blot-window");

        // ── In-window modal host (one per window) ──────────────────────────
        // All in-app dialogs are shown as overlays on this host instead of
        // separate toplevel windows. Cloned (Rc-backed) into the closures that
        // open dialogs below.
        let modal_host = modal_host::ModalHost::new();

        // ── Shared inbox state ─────────────────────────────────────────────
        let session: Rc<RefCell<NoteSession>> = Rc::new(RefCell::new(NoteSession::default()));

        // ── Shared workspace state ─────────────────────────────────────────
        let workspace_db: Rc<RefCell<Option<WorkspaceDb>>> = Rc::new(RefCell::new(None));
        let ws_session: Rc<RefCell<WorkspaceNoteSession>> =
            Rc::new(RefCell::new(WorkspaceNoteSession::default()));
        // Registry of known workspaces (owned in Rc so desk refresh can read it).
        let known_ws: Rc<RefCell<KnownWorkspaceRegistry>> = Rc::new(RefCell::new(
            KnownWorkspaceRegistry::load(&paths.known_workspaces),
        ));

        // ── Tab model ──────────────────────────────────────────────────────
        let tab_model: Rc<RefCell<TabModel>> = Rc::new(RefCell::new(TabModel::new()));

        // ── Status bar ─────────────────────────────────────────────────────
        let status_bar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        status_bar.add_css_class("status-bar");

        let mode_label = gtk::Label::new(Some("Editor"));
        mode_label.add_css_class("status-mode");
        mode_label.set_margin_start(10);
        mode_label.set_margin_end(8);

        let sep1 = gtk::Separator::new(gtk::Orientation::Vertical);
        sep1.set_margin_top(4);
        sep1.set_margin_bottom(4);

        let location_label = gtk::Label::new(Some("Inbox"));
        location_label.add_css_class("status-location");
        location_label.set_margin_start(8);

        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);

        let save_label = gtk::Label::new(Some("New note"));
        save_label.add_css_class("status-save");
        save_label.set_margin_end(10);

        status_bar.append(&mode_label);
        status_bar.append(&sep1);
        status_bar.append(&location_label);
        status_bar.append(&spacer);
        status_bar.append(&save_label);

        // ── Editor shell (Inbox notes) ─────────────────────────────────────
        // on_place_note is wired after window/stack/ws_shell are built; see below.
        // We use an Rc<RefCell<Option<Box<dyn Fn>>>> deferred-init pattern so the
        // editor can be built now and the callback installed later.
        let deferred_place: std::rc::Rc<std::cell::RefCell<Option<Box<dyn Fn(String, String)>>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));
        // Deferred tab-update callback — installed after tab_bar is built.
        let deferred_note_saved: Rc<RefCell<Option<Box<dyn Fn(String, String)>>>> =
            Rc::new(RefCell::new(None));
        let editor = {
            let deferred = deferred_place.clone();
            let deferred_saved = deferred_note_saved.clone();
            editor_shell::build(
                db.clone(),
                session.clone(),
                save_label.clone(),
                move |id, title| {
                    if let Some(f) = deferred.borrow().as_ref() {
                        f(id, title);
                    }
                },
                move |note_id, title| {
                    if let Some(f) = deferred_saved.borrow().as_ref() {
                        f(note_id, title);
                    }
                },
            )
        };

        // ── Workspace shell ────────────────────────────────────────────────
        let ws_shell = WorkspaceShell::new(
            workspace_db.clone(),
            ws_session.clone(),
            save_label.clone(),
            location_label.clone(),
        );
        let water_shell = WaterWorkspaceShell::new(save_label.clone(), location_label.clone());

        // ── External file shell (.txt / .md / .markdown) ───────────────────
        let external_shell = external_file_shell::build(
            save_label.clone(),
            location_label.clone(),
            modal_host.clone(),
        );

        // ── Mode stack ─────────────────────────────────────────────────────
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.set_hexpand(true);
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        stack.set_transition_duration(100);

        // ── Desk shell ─────────────────────────────────────────────────────
        let desk = {
            // on_return_to_editor
            let stack_for_return = stack.clone();
            let mode_for_return = mode_label.clone();

            // on_open_inbox_note
            let editor_for_open = editor.clone();
            let session_for_open = session.clone();
            let db_for_open = db.clone();
            let stack_for_open = stack.clone();
            let mode_for_open = mode_label.clone();

            // on_new_inbox_note
            let editor_for_new = editor.clone();
            let session_for_new = session.clone();
            let db_for_new = db.clone();
            let stack_for_new = stack.clone();
            let mode_for_new = mode_label.clone();

            // on_open_workspace
            let workspace_db_for_open = workspace_db.clone();
            let ws_shell_for_open = ws_shell.clone();
            let water_shell_for_open = water_shell.clone();
            let known_ws_for_open = known_ws.clone();
            let stack_for_ws_open = stack.clone();
            let mode_for_ws_open = mode_label.clone();
            let known_ws_path = paths.known_workspaces.clone();

            // on_new_workspace
            let workspace_db_for_new = workspace_db.clone();
            let ws_shell_for_new = ws_shell.clone();
            let known_ws_for_new = known_ws.clone();
            let stack_for_ws_new = stack.clone();
            let mode_for_ws_new = mode_label.clone();
            let known_ws_path_new = paths.known_workspaces.clone();

            // on_open_workspace_note
            let ws_shell_for_note = ws_shell.clone();
            let stack_for_note = stack.clone();
            let mode_for_note = mode_label.clone();

            // on_place_inbox_note
            let deferred_place_desk = deferred_place.clone();

            // on_new_workspace_note
            let ws_shell_for_wnew = ws_shell.clone();
            let stack_for_wnew = stack.clone();
            let mode_for_wnew = mode_label.clone();

            DeskShell::new(
                workspace_db.clone(),
                db.clone(),
                known_ws.clone(),
                // on_return_to_editor
                move || {
                    stack_for_return.set_visible_child_name("editor");
                    mode_for_return.set_text("Editor");
                },
                // on_open_inbox_note
                move |note_id| {
                    editor_for_open.force_save_sync(&db_for_open, &session_for_open);
                    let note_opt = db_for_open
                        .borrow()
                        .as_ref()
                        .and_then(|db| db.get_note(&note_id).ok().flatten());
                    if let Some(note) = note_opt {
                        editor_for_open.load_note(&note, &session_for_open);
                    }
                    stack_for_open.set_visible_child_name("editor");
                    mode_for_open.set_text("Editor");
                },
                // on_new_inbox_note
                move || {
                    editor_for_new.force_save_sync(&db_for_new, &session_for_new);
                    editor_for_new.new_note(&session_for_new);
                    stack_for_new.set_visible_child_name("editor");
                    mode_for_new.set_text("Editor");
                },
                // on_place_inbox_note
                move |note_id, note_title| {
                    if let Some(f) = deferred_place_desk.borrow().as_ref() {
                        f(note_id, note_title);
                    }
                },
                // on_open_workspace
                move |path| {
                    open_workspace_from_path(
                        &path,
                        &workspace_db_for_open,
                        &ws_shell_for_open,
                        &water_shell_for_open,
                        &known_ws_for_open,
                        &known_ws_path,
                        &stack_for_ws_open,
                        &mode_for_ws_open,
                    );
                },
                // on_new_workspace
                move || {
                    show_new_workspace_dialog(
                        &workspace_db_for_new,
                        &ws_shell_for_new,
                        &known_ws_for_new,
                        &known_ws_path_new,
                        &stack_for_ws_new,
                        &mode_for_ws_new,
                    );
                },
                // on_open_workspace_note
                move |note_id| {
                    ws_shell_for_note.open_note(&note_id);
                    stack_for_note.set_visible_child_name("workspace");
                    mode_for_note.set_text("Workspace");
                },
                // on_new_workspace_note
                move || {
                    ws_shell_for_wnew.new_note_in_current_room();
                    stack_for_wnew.set_visible_child_name("workspace");
                    mode_for_wnew.set_text("Workspace");
                },
            )
        };

        // ── Search shell ───────────────────────────────────────────────────
        let search_shell = {
            let editor_for_search = editor.clone();
            let session_for_search = session.clone();
            let db_for_search = db.clone();
            let ws_shell_for_search = ws_shell.clone();
            let water_shell_for_search = water_shell.clone();
            let workspace_db_for_search = workspace_db.clone();
            let known_ws_for_search = known_ws.clone();
            let known_ws_path_search = paths.known_workspaces.clone();
            let stack_for_search = stack.clone();
            let mode_for_search = mode_label.clone();
            let deferred_place_search = deferred_place.clone();

            SearchShell::new(
                db.clone(),
                workspace_db.clone(),
                known_ws.clone(),
                move |target| {
                    match target {
                        SearchNoteTarget::InboxNote { note_id } => {
                            editor_for_search.force_save_sync(&db_for_search, &session_for_search);
                            let note_opt = db_for_search
                                .borrow()
                                .as_ref()
                                .and_then(|db| db.get_note(&note_id).ok().flatten());
                            if let Some(note) = note_opt {
                                editor_for_search.load_note(&note, &session_for_search);
                            }
                            stack_for_search.set_visible_child_name("editor");
                            mode_for_search.set_text("Editor");
                        }
                        SearchNoteTarget::WorkspaceNote {
                            note_id,
                            workspace_path,
                        } => {
                            // Open the workspace if it isn't already open (or is a different one).
                            let already_open = workspace_db_for_search
                                .borrow()
                                .as_ref()
                                .map(|db| db.path == workspace_path)
                                .unwrap_or(false);
                            if !already_open && workspace_path.exists() {
                                open_workspace_from_path(
                                    &workspace_path,
                                    &workspace_db_for_search,
                                    &ws_shell_for_search,
                                    &water_shell_for_search,
                                    &known_ws_for_search,
                                    &known_ws_path_search,
                                    &stack_for_search,
                                    &mode_for_search,
                                );
                            }
                            ws_shell_for_search.open_note(&note_id);
                            stack_for_search.set_visible_child_name("workspace");
                            mode_for_search.set_text("Workspace");
                        }
                    }
                },
                // on_place_inbox_note
                move |note_id, note_title| {
                    if let Some(f) = deferred_place_search.borrow().as_ref() {
                        f(note_id, note_title);
                    }
                },
            )
        };

        // ── Room Map shell ─────────────────────────────────────────────────
        let room_map_shell = {
            let ws_shell_nav = ws_shell.clone();
            let stack_nav = stack.clone();
            let mode_nav = mode_label.clone();
            RoomMapShell::new(workspace_db.clone(), modal_host.clone(), move |room_id| {
                ws_shell_nav.navigate_to_room(&room_id);
                stack_nav.set_visible_child_name("workspace");
                mode_nav.set_text("Workspace");
            })
        };

        // ── Compare shell ──────────────────────────────────────────────────
        let compare_shell = {
            let stack_exit = stack.clone();
            let mode_exit = mode_label.clone();
            CompareShell::new(db.clone(), workspace_db.clone(), modal_host.clone(), move || {
                stack_exit.set_visible_child_name("editor");
                mode_exit.set_text("Editor");
            })
        };

        stack.add_named(&editor.root, Some("editor"));
        stack.add_named(&desk.root, Some("desk"));
        stack.add_named(&ws_shell.root, Some("workspace"));
        stack.add_named(&water_shell.root, Some("water-workspace"));
        stack.add_named(&search_shell.root, Some("search"));
        stack.add_named(&room_map_shell.root, Some("room-map"));
        stack.add_named(&compare_shell.root, Some("compare"));
        stack.add_named(&external_shell.root, Some("external"));

        // ── Place Note shared callback ──────────────────────────────────────
        // Installed here (after window/stack/ws_shell/editor are all live).
        {
            let host_ref = modal_host.clone();
            let db_ref = db.clone();
            let workspace_db_ref = workspace_db.clone();
            let known_ws_ref = known_ws.clone();
            let ws_shell_ref = ws_shell.clone();
            let water_shell_ref = water_shell.clone();
            let stack_ref = stack.clone();
            let mode_ref = mode_label.clone();
            let known_ws_path = paths.known_workspaces.clone();
            let editor_ref = editor.clone();
            let session_ref = session.clone();

            *deferred_place.borrow_mut() =
                Some(Box::new(move |note_id: String, note_title: String| {
                    let ws_db2 = workspace_db_ref.clone();
                    let known_ws2 = known_ws_ref.clone();
                    let ws_shell2 = ws_shell_ref.clone();
                    let water_shell2 = water_shell_ref.clone();
                    let stack2 = stack_ref.clone();
                    let mode2 = mode_ref.clone();
                    let path2 = known_ws_path.clone();
                    let editor2 = editor_ref.clone();
                    let session2 = session_ref.clone();

                    place_note_dialog::show(
                        &host_ref,
                        note_id,
                        note_title,
                        db_ref.clone(),
                        workspace_db_ref.clone(),
                        known_ws_ref.clone(),
                        move |placed_info| {
                            // Clear the editor — the inbox note is now archived.
                            editor2.new_note(&session2);
                            // Open the destination workspace and navigate to the placed note.
                            open_workspace_from_path(
                                &placed_info.workspace_path,
                                &ws_db2,
                                &ws_shell2,
                                &water_shell2,
                                &known_ws2,
                                &path2,
                                &stack2,
                                &mode2,
                            );
                            ws_shell2.open_note(&placed_info.workspace_note_id);
                            stack2.set_visible_child_name("workspace");
                            mode2.set_text("Workspace");
                        },
                    );
                }));
        }

        // ── Absorb-into-Blot callback ───────────────────────────────────────
        {
            let host_ref = modal_host.clone();
            let db_ref = db.clone();
            let workspace_db_ref = workspace_db.clone();
            let editor_ref = editor.clone();
            let session_ref = session.clone();
            let ws_shell_ref = ws_shell.clone();
            let stack_ref = stack.clone();
            let mode_ref = mode_label.clone();
            let save_ref = save_label.clone();
            let location_ref = location_label.clone();

            *external_shell.on_absorb.borrow_mut() = Some(Box::new(
                move |ef: crate::external_file::ExternalFile, title: String| {
                    let db2 = db_ref.clone();
                    let editor2 = editor_ref.clone();
                    let session2 = session_ref.clone();
                    let ws_shell2 = ws_shell_ref.clone();
                    let stack2 = stack_ref.clone();
                    let mode2 = mode_ref.clone();
                    let save2 = save_ref.clone();
                    let location2 = location_ref.clone();

                    absorb_dialog::show(
                        &host_ref,
                        ef,
                        title,
                        db_ref.clone(),
                        workspace_db_ref.clone(),
                        move |result| {
                            save2.set_text(&format!("Absorbed into {}", result.destination_label));
                            if result.target_kind == "inbox_note" {
                                let note_opt = db2
                                    .borrow()
                                    .as_ref()
                                    .and_then(|d| d.get_note(&result.target_id).ok().flatten());
                                if let Some(note) = note_opt {
                                    editor2.load_note(&note, &session2);
                                }
                                location2.set_text("Inbox");
                                stack2.set_visible_child_name("editor");
                                mode2.set_text("Editor");
                            } else {
                                ws_shell2.refresh();
                                ws_shell2.open_note(&result.target_id);
                                stack2.set_visible_child_name("workspace");
                                mode2.set_text("Workspace");
                            }
                        },
                    );
                },
            ));
        }

        // ── Tab bar ────────────────────────────────────────────────────────
        // Create an initial blank Inbox tab so the startup editor is tab-aware.
        tab_model.borrow_mut().new_blank(NoteSource::Inbox);

        let tab_bar = {
            let model_sw = tab_model.clone();
            let model_cl = tab_model.clone();
            let model_nw = tab_model.clone();

            let editor_sw = editor.clone();
            let session_sw = session.clone();
            let db_sw = db.clone();
            let ws_shell_sw = ws_shell.clone();
            let stack_sw = stack.clone();
            let mode_sw = mode_label.clone();

            let editor_cl = editor.clone();
            let session_cl = session.clone();
            let db_cl = db.clone();
            let ws_shell_cl = ws_shell.clone();
            let stack_cl = stack.clone();
            let mode_cl = mode_label.clone();

            let editor_nw = editor.clone();
            let session_nw = session.clone();
            let db_nw = db.clone();
            let stack_nw = stack.clone();
            let mode_nw = mode_label.clone();

            // We need the tab_bar Rc for refresh inside closures — create it up front.
            let tab_bar_rc: Rc<RefCell<Option<TabBar>>> = Rc::new(RefCell::new(None));
            let tb_for_sw = tab_bar_rc.clone();
            let tb_for_cl = tab_bar_rc.clone();
            let tb_for_nw = tab_bar_rc.clone();

            let on_switch = move |tab_id: String| {
                switch_to_tab(
                    &tab_id,
                    &model_sw,
                    &editor_sw,
                    &ws_shell_sw,
                    &session_sw,
                    &db_sw,
                    &stack_sw,
                    &mode_sw,
                );
                if let Some(tb) = tb_for_sw.borrow().as_ref() {
                    tb.refresh();
                }
            };

            let on_close = move |tab_id: String| {
                // Save the note before closing.
                let tab = model_cl.borrow().find_source_by_tab_id(&tab_id);
                if let Some(src) = tab {
                    match src {
                        NoteSource::Inbox => {
                            editor_cl.force_save_sync(&db_cl, &session_cl);
                        }
                        NoteSource::Workspace(_) => {
                            ws_shell_cl.force_save_sync();
                        }
                    }
                }
                let was_active = model_cl
                    .borrow()
                    .active_tab()
                    .map(|t| t.tab_id == tab_id)
                    .unwrap_or(false);
                model_cl.borrow_mut().close(&tab_id);
                // If we closed the active tab, load the new active tab.
                if was_active {
                    let new_active = model_cl.borrow().active_tab().map(|t| t.tab_id.clone());
                    if let Some(new_id) = new_active {
                        switch_to_tab(
                            &new_id,
                            &model_cl,
                            &editor_cl,
                            &ws_shell_cl,
                            &session_cl,
                            &db_cl,
                            &stack_cl,
                            &mode_cl,
                        );
                    }
                }
                if let Some(tb) = tb_for_cl.borrow().as_ref() {
                    tb.refresh();
                }
            };

            let on_new = move || {
                editor_nw.force_save_sync(&db_nw, &session_nw);
                editor_nw.new_note(&session_nw);
                let tab_id = model_nw.borrow_mut().new_blank(NoteSource::Inbox);
                stack_nw.set_visible_child_name("editor");
                mode_nw.set_text("Editor");
                if let Some(tb) = tb_for_nw.borrow().as_ref() {
                    tb.refresh();
                }
                let _ = tab_id;
            };

            let bar = TabBar::new(tab_model.clone(), on_switch, on_close, on_new);
            *tab_bar_rc.borrow_mut() = Some(bar.clone());
            (bar, tab_bar_rc)
        };
        let (tab_bar_widget, tab_bar_rc) = tab_bar;

        // Install the deferred on_note_saved callback now that tab_bar_widget exists.
        {
            let model = tab_model.clone();
            let bar = tab_bar_widget.clone();
            *deferred_note_saved.borrow_mut() =
                Some(Box::new(move |note_id: String, title: String| {
                    let mut m = model.borrow_mut();
                    m.update_active_note_id(&note_id);
                    m.update_active_title(&title);
                    drop(m);
                    bar.refresh();
                }));
        }

        // ── Editor action button callbacks ─────────────────────────────────

        // Split Note
        {
            let db2 = db.clone();
            let editor2 = editor.clone();
            let session2 = session.clone();
            let host2 = modal_host.clone();
            *editor.on_split.borrow_mut() = Some(Box::new(move |note_id: String| {
                // Get selected text from the editor body view.
                let buf = editor2.body_view.buffer();
                let selected = if let Some((start, end)) = buf.selection_bounds() {
                    buf.text(&start, &end, false).to_string()
                } else {
                    String::new()
                };
                if selected.trim().is_empty() {
                    host2.show_error(
                        "Split Note",
                        "Select some text in the note first, then split it into a new note.",
                    );
                    return;
                }
                let note_opt = db2
                    .borrow()
                    .as_ref()
                    .and_then(|d| d.get_note(&note_id).ok().flatten());
                let Some(note) = note_opt else { return };
                let db3 = db2.clone();
                let editor3 = editor2.clone();
                let session3 = session2.clone();
                let split_result = {
                    let guard = db3.borrow();
                    guard
                        .as_ref()
                        .map(|d| crate::ops::split_inbox_note(d, &note, &selected))
                };
                if let Some(outcome) = split_result {
                    match outcome {
                        Ok(result) => {
                            // Update original note body in the editor.
                            let loading = editor3.loading_flag.clone();
                            loading.set(true);
                            editor3
                                .body_view
                                .buffer()
                                .set_text(&result.updated_original_body);
                            loading.set(false);
                            // Force-save the updated original.
                            editor3.force_save_sync(&db3, &session3);
                            eprintln!(
                                "blot: Split Note → new note '{}' created",
                                result.new_note.title
                            );
                        }
                        Err(e) => {
                            eprintln!("blot: split error: {e}");
                            host2.show_error("Split Note", &format!("Could not split note: {e}"));
                        }
                    }
                }
            }));
        }

        // Bookmark Version
        {
            let db2 = db.clone();
            let host2 = modal_host.clone();
            *editor.on_bookmark.borrow_mut() = Some(Box::new(move |note_id: String| {
                let db3 = db2.clone();
                let note_opt = db3
                    .borrow()
                    .as_ref()
                    .and_then(|d| d.get_note(&note_id).ok().flatten());
                let Some(note) = note_opt else { return };
                version_history_shell::prompt_bookmark_name(&host2, move |name| {
                    if let Some(d) = db3.borrow().as_ref() {
                        match d.create_version(
                            &note,
                            "manual bookmark",
                            true,
                            Some(&name),
                            Some("manual"),
                            None,
                        ) {
                            Ok(_) => eprintln!("blot: Bookmarked version '{name}'"),
                            Err(e) => eprintln!("blot: bookmark error: {e}"),
                        }
                    }
                });
            }));
        }

        // Show Version History
        {
            let db2 = db.clone();
            let editor2 = editor.clone();
            let session2 = session.clone();
            let host2 = modal_host.clone();
            *editor.on_history.borrow_mut() = Some(Box::new(move |note_id: String| {
                let db3 = db2.clone();
                let editor3 = editor2.clone();
                let session3 = session2.clone();
                version_history_shell::open_inbox(
                    &host2,
                    db3.clone(),
                    &note_id,
                    move |restored| {
                        editor3.load_note(&restored, &session3);
                        eprintln!("blot: Version restored");
                    },
                );
            }));
        }

        // Merge Notes
        {
            let db2 = db.clone();
            let editor2 = editor.clone();
            let session2 = session.clone();
            let host2 = modal_host.clone();
            *editor.on_merge.borrow_mut() = Some(Box::new(move |note_id: String| {
                let db3 = db2.clone();
                let editor3 = editor2.clone();
                let session3 = session2.clone();
                let target_id = note_id.clone();
                merge_dialog::open_inbox(&host2, db3.clone(), &note_id, move |source_ids| {
                    let target_opt = db3
                        .borrow()
                        .as_ref()
                        .and_then(|d| d.get_note(&target_id).ok().flatten());
                    let Some(target) = target_opt else { return };
                    let sources: Vec<crate::inbox::InboxNote> = source_ids
                        .iter()
                        .filter_map(|id| {
                            db3.borrow()
                                .as_ref()
                                .and_then(|d| d.get_note(id).ok().flatten())
                        })
                        .collect();
                    let source_refs: Vec<&crate::inbox::InboxNote> = sources.iter().collect();
                    let op_id = crate::inbox::new_note_id();
                    if let Some(d) = db3.borrow().as_ref() {
                        match crate::ops::merge_inbox_notes(d, &target, &source_refs, &op_id) {
                            Ok(merged_body) => {
                                // Update editor with merged body.
                                let loading = editor3.loading_flag.clone();
                                loading.set(true);
                                editor3.body_view.buffer().set_text(&merged_body);
                                loading.set(false);
                                editor3.force_save_sync(&db3, &session3);
                                eprintln!("blot: Merged {} notes into current", sources.len());
                            }
                            Err(e) => eprintln!("blot: merge error: {e}"),
                        }
                    }
                });
            }));
        }

        // Initial tab bar render.
        tab_bar_widget.refresh();

        // ── Header bar ─────────────────────────────────────────────────────
        let header = gtk::HeaderBar::new();
        header.add_css_class("blot-header");
        header.set_show_title_buttons(true);

        let nav_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        nav_box.add_css_class("mode-nav-box");

        let desk_btn = gtk::Button::with_label("Desk");
        desk_btn.add_css_class("mode-button");
        desk_btn.set_tooltip_text(Some("Open Desk (Inbox, recents, workspaces)"));

        let search_btn = gtk::Button::with_label("Search");
        search_btn.add_css_class("mode-button");
        search_btn.set_tooltip_text(Some("Search notes (Ctrl+F)"));

        let room_map_btn = gtk::Button::with_label("Room Map");
        room_map_btn.add_css_class("mode-button");
        room_map_btn.set_tooltip_text(Some("View rooms and connections"));

        let workspace_btn = gtk::Button::with_label("Workspace");
        workspace_btn.add_css_class("mode-button");
        workspace_btn.set_tooltip_text(Some("Switch to focused workspace (Ctrl+W)"));

        let compare_btn = gtk::Button::with_label("Compare");
        compare_btn.add_css_class("mode-button");
        compare_btn.set_tooltip_text(Some("Compare two notes side by side"));

        nav_box.append(&desk_btn);
        nav_box.append(&search_btn);
        nav_box.append(&room_map_btn);
        nav_box.append(&workspace_btn);
        nav_box.append(&compare_btn);
        header.pack_start(&nav_box);

        let palette_btn = gtk::Button::with_label("Commands");
        palette_btn.add_css_class("mode-button");
        palette_btn.add_css_class("command-palette-btn");
        palette_btn.set_tooltip_text(Some("Command palette (Ctrl+Shift+P)"));
        header.pack_end(&palette_btn);

        window.set_titlebar(Some(&header));

        // ── Root layout ────────────────────────────────────────────────────
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&tab_bar_widget.root);
        root.append(&stack);
        root.append(&status_bar);
        // The modal host's overlay wraps the whole UI; `root` is its main child
        // and any dialog appears as a centered overlay above it.
        modal_host.overlay.set_child(Some(&root));
        window.set_child(Some(&modal_host.overlay));

        // ── Mode button connections ─────────────────────────────────────────

        {
            let stack = stack.clone();
            let mode_label = mode_label.clone();
            let editor = editor.clone();
            let session = session.clone();
            let db = db.clone();
            desk_btn.connect_clicked(move |_| {
                editor.force_save_sync(&db, &session);
                stack.set_visible_child_name("desk");
                mode_label.set_text("Desk");
            });
        }
        {
            let stack = stack.clone();
            let mode_label = mode_label.clone();
            let search_shell2 = search_shell.clone();
            search_btn.connect_clicked(move |_| {
                stack.set_visible_child_name("search");
                mode_label.set_text("Search");
                search_shell2.activate();
            });
        }
        {
            let stack = stack.clone();
            let mode_label = mode_label.clone();
            let rm_shell = room_map_shell.clone();
            room_map_btn.connect_clicked(move |_| {
                rm_shell.refresh();
                stack.set_visible_child_name("room-map");
                mode_label.set_text("Room Map");
            });
        }
        {
            let stack = stack.clone();
            let mode_label = mode_label.clone();
            let ws_shell2 = ws_shell.clone();
            let water_shell2 = water_shell.clone();
            let editor2 = editor.clone();
            let session2 = session.clone();
            let db2 = db.clone();
            workspace_btn.connect_clicked(move |_| {
                editor2.force_save_sync(&db2, &session2);
                ws_shell2.refresh();
                water_shell2.refresh();
                stack.set_visible_child_name("workspace");
                mode_label.set_text("Workspace");
            });
        }

        {
            let stack = stack.clone();
            let mode_label = mode_label.clone();
            let editor = editor.clone();
            let session = session.clone();
            let db = db.clone();
            let compare_shell2 = compare_shell.clone();
            compare_btn.connect_clicked(move |_| {
                // Save current note before entering compare mode.
                editor.force_save_sync(&db, &session);
                // Load the current editor note into the left panel if possible.
                if let Some(note_id) = session.borrow().note_id.clone() {
                    let note = db
                        .borrow()
                        .as_ref()
                        .and_then(|db| db.get_note(&note_id).ok().flatten());
                    if let Some(note) = note {
                        compare_shell2.load_left_inbox(&note);
                    }
                }
                stack.set_visible_child_name("compare");
                mode_label.set_text("Compare");
            });
        }

        // Refresh mode surfaces when they become visible.
        {
            let desk = desk.clone();
            let ws_shell2 = ws_shell.clone();
            let water_shell2 = water_shell.clone();
            let search_shell3 = search_shell.clone();
            let rm_shell2 = room_map_shell.clone();
            stack.connect_notify_local(Some("visible-child-name"), move |s, _| {
                match s.visible_child_name().as_deref() {
                    Some("desk") => desk.refresh(),
                    Some("workspace") => ws_shell2.refresh(),
                    Some("water-workspace") => water_shell2.refresh(),
                    Some("search") => search_shell3.activate(),
                    Some("room-map") => rm_shell2.refresh(),
                    _ => {}
                }
            });
        }

        // ── Command palette ────────────────────────────────────────────────
        {
            let host_ref = modal_host.clone();
            let save_ref = save_label.clone();
            let deferred_place_palette = deferred_place.clone();
            let editor_palette = editor.clone();
            let session_palette = session.clone();
            let rm_shell_palette = room_map_shell.clone();
            let stack_palette = stack.clone();
            let mode_palette = mode_label.clone();
            let gen_editor = editor.clone();
            let gen_session = session.clone();
            let gen_db = db.clone();
            let gen_ws = ws_shell.clone();
            let gen_stack = stack.clone();
            let gen_mode = mode_label.clone();
            let gen_tab_model = tab_model.clone();
            let gen_tab_bar = tab_bar_widget.clone();
            let gen_compare = compare_shell.clone();
            let gen_external = external_shell.clone();
            let gen_app = app.clone();
            let gen_config = config.clone();
            let gen_paths = paths.clone();
            palette_btn.connect_clicked(move |_| {
                let on_place: Option<std::rc::Rc<dyn Fn()>> = {
                    let note_id = session_palette.borrow().note_id.clone();
                    note_id.map(|id| {
                        let deferred = deferred_place_palette.clone();
                        let title = editor_palette.title_entry.text().to_string();
                        std::rc::Rc::new(move || {
                            if let Some(f) = deferred.borrow().as_ref() {
                                f(id.clone(), title.clone());
                            }
                        }) as std::rc::Rc<dyn Fn()>
                    })
                };
                let on_room_map =
                    make_room_map_cmd_handler(&rm_shell_palette, &stack_palette, &mode_palette);
                let on_general = make_general_cmd_handler(
                    &gen_editor,
                    &gen_session,
                    &gen_db,
                    &gen_ws,
                    &gen_stack,
                    &gen_mode,
                    &gen_tab_model,
                    &gen_tab_bar,
                    &gen_compare,
                    &gen_external,
                    &gen_app,
                    &gen_config,
                    &gen_paths,
                );
                command_palette::open(
                    &host_ref,
                    &save_ref,
                    on_place,
                    Some(on_room_map),
                    Some(on_general),
                );
            });
        }

        // ── Window actions / keyboard shortcuts ────────────────────────────

        // Ctrl+Shift+P → command palette
        {
            let host_action = modal_host.clone();
            let lbl = save_label.clone();
            let deferred_place_action = deferred_place.clone();
            let editor_action = editor.clone();
            let session_action = session.clone();
            let rm_shell_action = room_map_shell.clone();
            let stack_action = stack.clone();
            let mode_action = mode_label.clone();
            let gen_editor = editor.clone();
            let gen_session = session.clone();
            let gen_db = db.clone();
            let gen_ws = ws_shell.clone();
            let gen_stack = stack.clone();
            let gen_mode = mode_label.clone();
            let gen_tab_model = tab_model.clone();
            let gen_tab_bar = tab_bar_widget.clone();
            let gen_compare = compare_shell.clone();
            let gen_external = external_shell.clone();
            let gen_app = app.clone();
            let gen_config = config.clone();
            let gen_paths = paths.clone();
            let action = gio::SimpleAction::new("open-command-palette", None);
            action.connect_activate(move |_, _| {
                let on_place: Option<std::rc::Rc<dyn Fn()>> = {
                    let note_id = session_action.borrow().note_id.clone();
                    note_id.map(|id| {
                        let deferred = deferred_place_action.clone();
                        let title = editor_action.title_entry.text().to_string();
                        std::rc::Rc::new(move || {
                            if let Some(f) = deferred.borrow().as_ref() {
                                f(id.clone(), title.clone());
                            }
                        }) as std::rc::Rc<dyn Fn()>
                    })
                };
                let on_room_map =
                    make_room_map_cmd_handler(&rm_shell_action, &stack_action, &mode_action);
                let on_general = make_general_cmd_handler(
                    &gen_editor,
                    &gen_session,
                    &gen_db,
                    &gen_ws,
                    &gen_stack,
                    &gen_mode,
                    &gen_tab_model,
                    &gen_tab_bar,
                    &gen_compare,
                    &gen_external,
                    &gen_app,
                    &gen_config,
                    &gen_paths,
                );
                command_palette::open(
                    &host_action,
                    &lbl,
                    on_place,
                    Some(on_room_map),
                    Some(on_general),
                );
            });
            window.add_action(&action);
            app.set_accels_for_action("win.open-command-palette", &["<Ctrl><Shift>p"]);
        }

        // Ctrl+F → search mode
        {
            let s = stack.clone();
            let m = mode_label.clone();
            let ss = search_shell.clone();
            let action = gio::SimpleAction::new("open-search", None);
            action.connect_activate(move |_, _| {
                s.set_visible_child_name("search");
                m.set_text("Search");
                ss.activate();
            });
            window.add_action(&action);
            app.set_accels_for_action("win.open-search", &["<Ctrl>f"]);
        }

        // Ctrl+D → desk mode
        {
            let s = stack.clone();
            let m = mode_label.clone();
            let ed = editor.clone();
            let sess = session.clone();
            let db2 = db.clone();
            let action = gio::SimpleAction::new("open-desk", None);
            action.connect_activate(move |_, _| {
                ed.force_save_sync(&db2, &sess);
                s.set_visible_child_name("desk");
                m.set_text("Desk");
            });
            window.add_action(&action);
            app.set_accels_for_action("win.open-desk", &["<Ctrl>d"]);
        }

        // Ctrl+N / Ctrl+T → new Inbox note in a new tab
        {
            let s = stack.clone();
            let m = mode_label.clone();
            let ed = editor.clone();
            let sess = session.clone();
            let db2 = db.clone();
            let tm = tab_model.clone();
            let tb = tab_bar_widget.clone();
            let action = gio::SimpleAction::new("new-inbox-note", None);
            action.connect_activate(move |_, _| {
                ed.force_save_sync(&db2, &sess);
                ed.new_note(&sess);
                tm.borrow_mut().new_blank(NoteSource::Inbox);
                tb.refresh();
                s.set_visible_child_name("editor");
                m.set_text("Editor");
            });
            window.add_action(&action);
            app.set_accels_for_action("win.new-inbox-note", &["<Ctrl>n", "<Ctrl>t"]);
        }

        // Ctrl+W → workspace mode
        {
            let s = stack.clone();
            let m = mode_label.clone();
            let ed = editor.clone();
            let sess = session.clone();
            let db2 = db.clone();
            let ws2 = ws_shell.clone();
            let water_shell2 = water_shell.clone();
            let action = gio::SimpleAction::new("open-workspace", None);
            action.connect_activate(move |_, _| {
                ed.force_save_sync(&db2, &sess);
                ws2.refresh();
                water_shell2.refresh();
                s.set_visible_child_name("workspace");
                m.set_text("Workspace");
            });
            window.add_action(&action);
            app.set_accels_for_action("win.open-workspace", &["<Ctrl>w"]);
        }

        // Ctrl+Shift+N → new workspace note
        {
            let ws2 = ws_shell.clone();
            let s = stack.clone();
            let m = mode_label.clone();
            let action = gio::SimpleAction::new("new-workspace-note", None);
            action.connect_activate(move |_, _| {
                ws2.new_note_in_current_room();
                s.set_visible_child_name("workspace");
                m.set_text("Workspace");
            });
            window.add_action(&action);
            app.set_accels_for_action("win.new-workspace-note", &["<Ctrl><Shift>n"]);
        }

        // Ctrl+Page Down → next tab
        {
            let ed = editor.clone();
            let sess = session.clone();
            let db2 = db.clone();
            let ws2 = ws_shell.clone();
            let s = stack.clone();
            let m = mode_label.clone();
            let tm = tab_model.clone();
            let tb = tab_bar_widget.clone();
            let action = gio::SimpleAction::new("next-tab", None);
            action.connect_activate(move |_, _| {
                ed.force_save_sync(&db2, &sess);
                tm.borrow_mut().next_tab();
                let id = tm.borrow().active_tab().map(|t| t.tab_id.clone());
                if let Some(id) = id {
                    switch_to_tab(&id, &tm, &ed, &ws2, &sess, &db2, &s, &m);
                    tb.refresh();
                }
            });
            window.add_action(&action);
            app.set_accels_for_action("win.next-tab", &["<Ctrl>Page_Down"]);
        }

        // Ctrl+Page Up → previous tab
        {
            let ed = editor.clone();
            let sess = session.clone();
            let db2 = db.clone();
            let ws2 = ws_shell.clone();
            let s = stack.clone();
            let m = mode_label.clone();
            let tm = tab_model.clone();
            let tb = tab_bar_widget.clone();
            let action = gio::SimpleAction::new("prev-tab", None);
            action.connect_activate(move |_, _| {
                ed.force_save_sync(&db2, &sess);
                tm.borrow_mut().prev_tab();
                let id = tm.borrow().active_tab().map(|t| t.tab_id.clone());
                if let Some(id) = id {
                    switch_to_tab(&id, &tm, &ed, &ws2, &sess, &db2, &s, &m);
                    tb.refresh();
                }
            });
            window.add_action(&action);
            app.set_accels_for_action("win.prev-tab", &["<Ctrl>Page_Up"]);
        }

        // Ctrl+Alt+N → new window
        {
            let app2 = app.clone();
            let db2 = db.clone();
            let config2 = config.clone();
            let paths2 = paths.clone();
            let action = gio::SimpleAction::new("new-window", None);
            action.connect_activate(move |_, _| {
                crate::app::open_new_window(&app2, db2.clone(), config2.clone(), paths2.clone());
            });
            window.add_action(&action);
            app.set_accels_for_action("win.new-window", &["<Ctrl><Alt>n"]);
        }

        // Escape → back to editor
        {
            let s = stack.clone();
            let m = mode_label.clone();
            let action = gio::SimpleAction::new("back-to-editor", None);
            action.connect_activate(move |_, _| {
                s.set_visible_child_name("editor");
                m.set_text("Editor");
            });
            window.add_action(&action);
            app.set_accels_for_action("win.back-to-editor", &["Escape"]);
        }

        // ── Save on close ──────────────────────────────────────────────────
        // Inbox/workspace notes autosave silently. External files use manual
        // save, so unsaved external edits must NOT be lost silently: prompt to
        // Save / Discard / Cancel before closing.
        {
            let editor = editor.clone();
            let session = session.clone();
            let db = db.clone();
            let ws_shell2 = ws_shell.clone();
            let water_shell2 = water_shell.clone();
            let compare_shell2 = compare_shell.clone();
            let external_shell2 = external_shell.clone();
            let window_close = window.clone();
            let force_close = std::rc::Rc::new(std::cell::Cell::new(false));
            window.connect_close_request(move |_| {
                editor.force_save_sync(&db, &session);
                ws_shell2.force_save_sync();
                water_shell2.force_save_sync();
                compare_shell2.force_save_both();

                if external_shell2.has_unsaved_changes() && !force_close.get() {
                    let shell = external_shell2.clone();
                    let fc = force_close.clone();
                    let win = window_close.clone();
                    let dialog = gtk::AlertDialog::builder()
                        .message("Unsaved changes to file")
                        .detail(
                            "This external file has unsaved edits. Save them back to \
                             the file before closing?",
                        )
                        .buttons(["Cancel", "Discard", "Save"])
                        .default_button(2)
                        .cancel_button(0)
                        .modal(true)
                        .build();
                    dialog.choose(Some(&window_close), None::<&gio::Cancellable>, move |res| {
                        match res {
                            Ok(2) => {
                                shell.save_sync();
                                fc.set(true);
                                win.close();
                            }
                            Ok(1) => {
                                fc.set(true);
                                win.close();
                            }
                            _ => { /* Cancel: stay open */ }
                        }
                    });
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            });
        }

        // ── Apply launch config ────────────────────────────────────────────
        apply_launch_config(
            &stack,
            &mode_label,
            &location_label,
            launch,
            &workspace_db,
            &ws_shell,
            &water_shell,
            &known_ws,
            &paths.known_workspaces,
            &search_shell,
            &external_shell,
        );

        window
    }
}

// ─── Workspace open/create helpers ────────────────────────────────────────────

/// Open a workspace from a path. Registers it in the known workspaces list and
/// switches to Workspace Mode.
pub fn open_workspace_from_path(
    path: &std::path::Path,
    workspace_db: &Rc<RefCell<Option<WorkspaceDb>>>,
    ws_shell: &WorkspaceShell,
    water_shell: &WaterWorkspaceShell,
    known_ws: &Rc<RefCell<KnownWorkspaceRegistry>>,
    _known_ws_file: &std::path::Path, // reserved for future registry path threading
    stack: &gtk::Stack,
    mode_label: &gtk::Label,
) {
    match water_shell.open_path(path) {
        Ok(name) => {
            *workspace_db.borrow_mut() = None;
            stack.set_visible_child_name("water-workspace");
            mode_label.set_text("Workspace");
            register_workspace(known_ws, path, &name);
            return;
        }
        Err(crate::water_file::WaterFileError::Malformed(_)) => {
            // A legacy SQLite workspace is not JSON; try the older shell below.
        }
        Err(e) => {
            eprintln!("blot: failed to open .water file {}: {e}", path.display());
            show_error_dialog(
                None::<&gtk::Window>,
                "Could not open .water file",
                &format!("{e}"),
            );
            return;
        }
    }

    match WorkspaceDb::open(path) {
        Ok(db) => {
            let name = db.workspace_name();
            *workspace_db.borrow_mut() = Some(db);
            ws_shell.refresh();
            stack.set_visible_child_name("workspace");
            mode_label.set_text("Workspace");
            register_workspace(known_ws, path, &name);
        }
        Err(e) => {
            eprintln!("blot: failed to open workspace {}: {e}", path.display());
            show_error_dialog(
                None::<&gtk::Window>,
                "Could not open workspace",
                &format!("{e}"),
            );
        }
    }
}

fn register_workspace(
    known_ws: &Rc<RefCell<KnownWorkspaceRegistry>>,
    path: &std::path::Path,
    display_name: &str,
) {
    let now = now_iso8601();
    known_ws.borrow_mut().add_or_update(KnownWorkspace {
        path: path.to_path_buf(),
        display_name: display_name.to_string(),
        last_opened_at: now.clone(),
        last_focused_at: now,
        last_room_id: None,
        last_note_id: None,
        last_container_kind: None,
        last_container_id: None,
    });
}

/// Show a file-chooser dialog to create a new workspace, then open it.
fn show_new_workspace_dialog(
    workspace_db: &Rc<RefCell<Option<WorkspaceDb>>>,
    ws_shell: &WorkspaceShell,
    known_ws: &Rc<RefCell<KnownWorkspaceRegistry>>,
    _known_ws_file: &std::path::Path, // reserved for future registry path threading
    stack: &gtk::Stack,
    mode_label: &gtk::Label,
) {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Watercolor Workspace (*.water)"));
    filter.add_pattern("*.water");

    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);

    let dialog = gtk::FileDialog::builder()
        .title("Create New Workspace")
        .accept_label("Create")
        .initial_name("Workspace.water")
        .modal(true)
        .filters(&filters)
        .default_filter(&filter)
        .build();

    let workspace_db = workspace_db.clone();
    let ws_shell = ws_shell.clone();
    let known_ws = known_ws.clone();
    let stack = stack.clone();
    let mode_label = mode_label.clone();

    dialog.save(
        None::<&gtk::Window>,
        None::<&gio::Cancellable>,
        move |result| {
            if let Ok(file) = result {
                if let Some(mut path) = file.path() {
                    // Ensure .water extension.
                    if path.extension().map_or(true, |e| e != "water") {
                        let stem = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "Workspace".to_string());
                        path = path.with_file_name(format!("{stem}.water"));
                    }

                    let ws_name = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "Workspace".to_string());

                    match WorkspaceDb::create_new(&path, &ws_name) {
                        Ok(db) => {
                            let name = db.workspace_name();
                            *workspace_db.borrow_mut() = Some(db);
                            ws_shell.refresh();
                            stack.set_visible_child_name("workspace");
                            mode_label.set_text("Workspace");
                            register_workspace(&known_ws, &path, &name);
                        }
                        Err(e) => {
                            eprintln!("blot: failed to create workspace: {e}");
                            show_error_dialog(
                                None::<&gtk::Window>,
                                "Could not create workspace",
                                &format!("{e}"),
                            );
                        }
                    }
                }
            }
        },
    );
}

/// Build the Room Map command handler passed to the command palette.
/// Handles: Open Room Map, Create Room, Connect Rooms, Open Selected Room,
/// Change Room Connection Type, Remove Room Connection.
fn make_room_map_cmd_handler(
    room_map_shell: &RoomMapShell,
    stack: &gtk::Stack,
    mode_label: &gtk::Label,
) -> std::rc::Rc<dyn Fn(&str)> {
    let rm = room_map_shell.clone();
    let s = stack.clone();
    let m = mode_label.clone();
    std::rc::Rc::new(move |cmd: &str| {
        match cmd {
            "Open Room Map" => {
                rm.refresh();
                s.set_visible_child_name("room-map");
                m.set_text("Room Map");
            }
            "Create Room" => {
                rm.refresh();
                s.set_visible_child_name("room-map");
                m.set_text("Room Map");
                // Trigger the add-room dialog via a tiny button lookup isn't straightforward
                // from here, so we just navigate and the user can click + Room.
                eprintln!("blot: Create Room — Room Map opened, click + Room to add");
            }
            "Connect Rooms" => {
                rm.refresh();
                s.set_visible_child_name("room-map");
                m.set_text("Room Map");
                eprintln!("blot: Connect Rooms — Room Map opened, click + Connect to add a Door");
            }
            "Open Selected Room" => {
                // The on_open_room callback inside the shell handles the actual navigation.
                // Here we just ensure room map is visible so the user can double-click.
                rm.refresh();
                s.set_visible_child_name("room-map");
                m.set_text("Room Map");
                eprintln!(
                    "blot: Open Selected Room — double-click a Room card in the map to open it"
                );
            }
            other => {
                eprintln!("blot: room map command '{other}' — use Room Map UI");
            }
        }
    })
}

fn show_error_dialog(parent: Option<&gtk::Window>, title: &str, message: &str) {
    let dialog = gtk::AlertDialog::builder()
        .message(title)
        .detail(message)
        .buttons(["OK"])
        .default_button(0)
        .cancel_button(0)
        .modal(true)
        .build();
    dialog.choose(parent, None::<&gio::Cancellable>, |_| {});
}

// ─── Tab switching ────────────────────────────────────────────────────────────

/// Make the given tab active, then load its note into the correct editor surface.
fn switch_to_tab(
    tab_id: &str,
    model: &Rc<RefCell<TabModel>>,
    editor: &editor_shell::EditorWidgets,
    ws_shell: &WorkspaceShell,
    session: &Rc<RefCell<crate::inbox::NoteSession>>,
    db: &Rc<RefCell<Option<InboxDb>>>,
    stack: &gtk::Stack,
    mode_label: &gtk::Label,
) {
    model.borrow_mut().set_active_by_id(tab_id);
    let tab = model.borrow().active_tab().cloned();
    let Some(tab) = tab else {
        return;
    };
    match tab.source {
        NoteSource::Inbox => {
            if let Some(note_id) = tab.note_id.as_deref() {
                let note = db
                    .borrow()
                    .as_ref()
                    .and_then(|d| d.get_note(note_id).ok().flatten());
                if let Some(note) = note {
                    editor.load_note(&note, session);
                } else {
                    editor.new_note(session);
                }
            } else {
                editor.new_note(session);
            }
            stack.set_visible_child_name("editor");
            mode_label.set_text("Editor");
        }
        NoteSource::Workspace(_) => {
            if let Some(note_id) = tab.note_id.as_deref() {
                ws_shell.open_note(note_id);
            }
            stack.set_visible_child_name("workspace");
            mode_label.set_text("Workspace");
        }
    }
}

// ─── General command palette handler ─────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn make_general_cmd_handler(
    editor: &editor_shell::EditorWidgets,
    session: &Rc<RefCell<crate::inbox::NoteSession>>,
    db: &Rc<RefCell<Option<InboxDb>>>,
    ws_shell: &WorkspaceShell,
    stack: &gtk::Stack,
    mode_label: &gtk::Label,
    tab_model: &Rc<RefCell<TabModel>>,
    tab_bar: &crate::ui::tab_bar::TabBar,
    compare: &CompareShell,
    external_shell: &ExternalFileShell,
    app: &Application,
    config: &Rc<AppConfig>,
    paths: &Rc<AppPaths>,
) -> std::rc::Rc<dyn Fn(&str)> {
    let ed = editor.clone();
    let sess = session.clone();
    let db = db.clone();
    let ws = ws_shell.clone();
    let s = stack.clone();
    let m = mode_label.clone();
    let tm = tab_model.clone();
    let tb = tab_bar.clone();
    let cmp = compare.clone();
    let ext = external_shell.clone();
    let app = app.clone();
    let cfg = config.clone();
    let pth = paths.clone();

    std::rc::Rc::new(move |cmd: &str| match cmd {
        "New Inbox Note" => {
            ed.force_save_sync(&db, &sess);
            ed.new_note(&sess);
            tm.borrow_mut().new_blank(NoteSource::Inbox);
            tb.refresh();
            s.set_visible_child_name("editor");
            m.set_text("Editor");
        }
        "New Workspace Note" => {
            ws.new_note_in_current_room();
            s.set_visible_child_name("workspace");
            m.set_text("Workspace");
        }
        "Close Tab" => {
            let (tab_id, src) = {
                let model = tm.borrow();
                match model.active_tab() {
                    Some(t) => (t.tab_id.clone(), Some(t.source.clone())),
                    None => return,
                }
            };
            match src {
                Some(NoteSource::Inbox) => ed.force_save_sync(&db, &sess),
                Some(NoteSource::Workspace(_)) => ws.force_save_sync(),
                None => {}
            }
            tm.borrow_mut().close(&tab_id);
            let new_id = tm.borrow().active_tab().map(|t| t.tab_id.clone());
            if let Some(id) = new_id {
                switch_to_tab(&id, &tm, &ed, &ws, &sess, &db, &s, &m);
            }
            tb.refresh();
        }
        "Next Tab" => {
            ed.force_save_sync(&db, &sess);
            tm.borrow_mut().next_tab();
            let id = tm.borrow().active_tab().map(|t| t.tab_id.clone());
            if let Some(id) = id {
                switch_to_tab(&id, &tm, &ed, &ws, &sess, &db, &s, &m);
                tb.refresh();
            }
        }
        "Previous Tab" => {
            ed.force_save_sync(&db, &sess);
            tm.borrow_mut().prev_tab();
            let id = tm.borrow().active_tab().map(|t| t.tab_id.clone());
            if let Some(id) = id {
                switch_to_tab(&id, &tm, &ed, &ws, &sess, &db, &s, &m);
                tb.refresh();
            }
        }
        "Open Current Note in New Window" | "New Window" => {
            crate::app::open_new_window(&app, db.clone(), cfg.clone(), pth.clone());
        }
        "Open Compare Mode" => {
            ed.force_save_sync(&db, &sess);
            if let Some(note_id) = sess.borrow().note_id.clone() {
                let note = db
                    .borrow()
                    .as_ref()
                    .and_then(|d| d.get_note(&note_id).ok().flatten());
                if let Some(note) = note {
                    cmp.load_left_inbox(&note);
                }
            }
            s.set_visible_child_name("compare");
            m.set_text("Compare");
        }
        "Split Note" => {
            let note_id = sess.borrow().note_id.clone();
            if let Some(nid) = note_id {
                if let Some(f) = ed.on_split.borrow().as_ref() {
                    f(nid);
                }
            }
        }
        "Bookmark Version" => {
            let note_id = sess.borrow().note_id.clone();
            if let Some(nid) = note_id {
                if let Some(f) = ed.on_bookmark.borrow().as_ref() {
                    f(nid);
                }
            }
        }
        "Show Version History" => {
            let note_id = sess.borrow().note_id.clone();
            if let Some(nid) = note_id {
                if let Some(f) = ed.on_history.borrow().as_ref() {
                    f(nid);
                }
            }
        }
        "Merge Notes" => {
            let note_id = sess.borrow().note_id.clone();
            if let Some(nid) = note_id {
                if let Some(f) = ed.on_merge.borrow().as_ref() {
                    f(nid);
                }
            }
        }
        "Absorb File" => {
            if ext.current.borrow().is_some() {
                ext.trigger_absorb();
            } else {
                eprintln!("blot: Absorb File — open a .txt/.md file first");
            }
        }
        other => {
            eprintln!("blot: general command '{other}' not handled");
        }
    })
}

// ─── Launch config ────────────────────────────────────────────────────────────

fn apply_launch_config(
    stack: &gtk::Stack,
    mode_label: &gtk::Label,
    location_label: &gtk::Label,
    launch: &LaunchConfig,
    workspace_db: &Rc<RefCell<Option<WorkspaceDb>>>,
    ws_shell: &WorkspaceShell,
    water_shell: &WaterWorkspaceShell,
    known_ws: &Rc<RefCell<KnownWorkspaceRegistry>>,
    _known_ws_file: &std::path::Path,
    search_shell: &SearchShell,
    external_shell: &ExternalFileShell,
) {
    // External plain-text / Markdown file takes precedence: open it directly.
    if let Some(path) = launch.external_file.as_ref() {
        match external_shell.open_path(path) {
            Ok(()) => {
                stack.set_visible_child_name("external");
                mode_label.set_text("Editor");
                location_label.set_text("External File");
            }
            Err(e) => {
                eprintln!("blot: could not open file {}: {e}", path.display());
                show_error_dialog(None::<&gtk::Window>, "Could not open file", &e);
            }
        }
        return;
    }

    if launch.room_map {
        stack.set_visible_child_name("room-map");
        mode_label.set_text("Room Map");
        return;
    }
    if let Some(ref q) = launch.search_query {
        stack.set_visible_child_name("search");
        mode_label.set_text("Search");
        search_shell.set_query_and_run(q);
        return;
    }
    if launch.inbox {
        stack.set_visible_child_name("desk");
        mode_label.set_text("Desk");
        location_label.set_text("Inbox");
        return;
    }

    // --workspace <path> or positional arg or --new-workspace-note <path>
    let ws_path = launch
        .workspace
        .as_ref()
        .or(launch.new_workspace_note.as_ref());

    if let Some(path) = ws_path {
        if !path.exists() {
            eprintln!(
                "blot: workspace file not found: {} — opening Inbox instead",
                path.display()
            );
            return;
        }
        open_workspace_from_path(
            path,
            workspace_db,
            ws_shell,
            water_shell,
            known_ws,
            _known_ws_file,
            stack,
            mode_label,
        );

        // --new-workspace-note remains available only for legacy SQLite
        // workspaces until JSON note creation is specified.
        if launch.new_workspace_note.is_some()
            && stack.visible_child_name().as_deref() == Some("workspace")
        {
            ws_shell.new_note_in_current_room();
        }
    }
    // Default: editor mode (stack initial child is "editor").
}
