use crate::config::AppConfig;
use crate::inbox::{InboxDb, NoteSession};
use crate::known_workspaces::{KnownWorkspace, KnownWorkspaceRegistry};
use crate::launch::LaunchConfig;
use crate::paths::AppPaths;
use crate::ui::desk_shell::DeskShell;
use crate::ui::search_shell::{SearchNoteTarget, SearchShell};
use crate::ui::water_workspace_shell::WaterWorkspaceShell;
use crate::ui::workspace_shell::WorkspaceShell;
use crate::ui::{command_palette, editor_shell, place_note_dialog, room_map_shell};
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
        _config: &AppConfig,
        paths: &AppPaths,
        db: Rc<RefCell<Option<InboxDb>>>,
    ) -> ApplicationWindow {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("Blot")
            .default_width(1100)
            .default_height(740)
            .build();
        window.add_css_class("blot-window");

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
        let editor = {
            let deferred = deferred_place.clone();
            editor_shell::build(
                db.clone(),
                session.clone(),
                save_label.clone(),
                move |id, title| {
                    if let Some(f) = deferred.borrow().as_ref() {
                        f(id, title);
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

        // ── Room Map placeholder ───────────────────────────────────────────
        let room_map = room_map_shell::build();

        stack.add_named(&editor.root, Some("editor"));
        stack.add_named(&desk.root, Some("desk"));
        stack.add_named(&ws_shell.root, Some("workspace"));
        stack.add_named(&water_shell.root, Some("water-workspace"));
        stack.add_named(&search_shell.root, Some("search"));
        stack.add_named(&room_map, Some("room-map"));

        // ── Place Note shared callback ──────────────────────────────────────
        // Installed here (after window/stack/ws_shell/editor are all live).
        {
            let window_ref = window.clone();
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
                        &window_ref,
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

        nav_box.append(&desk_btn);
        nav_box.append(&search_btn);
        nav_box.append(&room_map_btn);
        nav_box.append(&workspace_btn);
        header.pack_start(&nav_box);

        let palette_btn = gtk::Button::with_label("Commands");
        palette_btn.add_css_class("mode-button");
        palette_btn.add_css_class("command-palette-btn");
        palette_btn.set_tooltip_text(Some("Command palette (Ctrl+Shift+P)"));
        header.pack_end(&palette_btn);

        window.set_titlebar(Some(&header));

        // ── Root layout ────────────────────────────────────────────────────
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&stack);
        root.append(&status_bar);
        window.set_child(Some(&root));

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
            room_map_btn.connect_clicked(move |_| {
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

        // Refresh mode surfaces when they become visible.
        {
            let desk = desk.clone();
            let ws_shell2 = ws_shell.clone();
            let water_shell2 = water_shell.clone();
            let search_shell3 = search_shell.clone();
            stack.connect_notify_local(Some("visible-child-name"), move |s, _| {
                match s.visible_child_name().as_deref() {
                    Some("desk") => desk.refresh(),
                    Some("workspace") => ws_shell2.refresh(),
                    Some("water-workspace") => water_shell2.refresh(),
                    Some("search") => search_shell3.activate(),
                    _ => {}
                }
            });
        }

        // ── Command palette ────────────────────────────────────────────────
        {
            let window_ref = window.clone();
            let save_ref = save_label.clone();
            let deferred_place_palette = deferred_place.clone();
            let editor_palette = editor.clone();
            let session_palette = session.clone();
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
                command_palette::open(&window_ref, &save_ref, on_place);
            });
        }

        // ── Window actions / keyboard shortcuts ────────────────────────────

        // Ctrl+Shift+P → command palette
        {
            let w = window.clone();
            let lbl = save_label.clone();
            let deferred_place_action = deferred_place.clone();
            let editor_action = editor.clone();
            let session_action = session.clone();
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
                command_palette::open(&w, &lbl, on_place);
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

        // Ctrl+N → new Inbox note
        {
            let s = stack.clone();
            let m = mode_label.clone();
            let ed = editor.clone();
            let sess = session.clone();
            let db2 = db.clone();
            let action = gio::SimpleAction::new("new-inbox-note", None);
            action.connect_activate(move |_, _| {
                ed.force_save_sync(&db2, &sess);
                ed.new_note(&sess);
                s.set_visible_child_name("editor");
                m.set_text("Editor");
            });
            window.add_action(&action);
            app.set_accels_for_action("win.new-inbox-note", &["<Ctrl>n"]);
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
        {
            let editor = editor.clone();
            let session = session.clone();
            let db = db.clone();
            let ws_shell2 = ws_shell.clone();
            let water_shell2 = water_shell.clone();
            window.connect_close_request(move |_| {
                editor.force_save_sync(&db, &session);
                ws_shell2.force_save_sync();
                water_shell2.force_save_sync();
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
) {
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
