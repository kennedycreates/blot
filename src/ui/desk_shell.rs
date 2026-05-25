//! Desk Mode surface for Blot.
//!
//! A full-window mode for finding, reopening, organizing, and managing notes.
//! Three-panel layout: Left sidebar (Inbox / Pinned / Recent / Workspaces),
//! Center (focused-workspace browser), Right (Quick Actions).
//!
//! Blot still opens into Editor Mode by default. Desk is one click away.

use crate::inbox::{format_date_short, new_note_id, now_iso8601, InboxDb, PinEntry, RecentEntry};
use crate::known_workspaces::KnownWorkspaceRegistry;
use crate::workspace::{ContainerKind, WorkspaceDb};
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

// ── Internal snapshot types ────────────────────────────────────────────────────
// Collected while holding a workspace DB borrow, then released so UI can build.

#[derive(Debug, Clone)]
struct NoteSnap {
    id: String,
    title: String,
    snippet: String,
    updated_at: String,
}

#[derive(Debug, Clone)]
struct ContainerSnap {
    id: String,
    name: String,
    is_pile: bool,
    note_count: i64,
    notes: Vec<NoteSnap>,
}

#[derive(Debug, Clone)]
struct RoomSnap {
    id: String,
    name: String,
    shelves: Vec<ContainerSnap>,
    piles: Vec<ContainerSnap>,
    loose_notes: Vec<NoteSnap>,
    loose_count: i64,
}

#[derive(Debug, Clone)]
struct WsSnap {
    name: String,
    path: String,
    rooms: Vec<RoomSnap>,
}

// ── DeskShell ──────────────────────────────────────────────────────────────────

/// The Desk Mode surface. Clone is cheap — all inner types are reference-counted.
#[derive(Clone)]
pub struct DeskShell {
    /// Root widget added to the mode stack.
    pub root: gtk::Box,

    // ── Left panel list widgets (refreshed on refresh())
    inbox_list: gtk::ListBox,
    inbox_count_label: gtk::Label,
    inbox_empty_label: gtk::Label,
    pins_list: gtk::ListBox,
    pins_empty_label: gtk::Label,
    recent_list: gtk::ListBox,
    recent_empty_label: gtk::Label,
    ws_sidebar_list: gtk::ListBox,
    ws_sidebar_empty_label: gtk::Label,

    // ── Center panel container (cleared and rebuilt on refresh)
    center_content: gtk::Box,

    // ── Shared data
    workspace_db: Rc<RefCell<Option<WorkspaceDb>>>,
    inbox_db: Rc<RefCell<Option<InboxDb>>>,
    known_ws: Rc<RefCell<KnownWorkspaceRegistry>>,

    // ── Callbacks (Rc so they're cheap to clone into closures)
    on_open_inbox_note: Rc<dyn Fn(String)>,
    on_place_inbox_note: Rc<dyn Fn(String, String)>,
    on_open_workspace: Rc<dyn Fn(std::path::PathBuf)>,
    on_new_workspace: Rc<dyn Fn()>,
    on_open_workspace_note: Rc<dyn Fn(String)>,
    on_new_workspace_note: Rc<dyn Fn()>,
}

impl DeskShell {
    // ── Constructor ───────────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        workspace_db: Rc<RefCell<Option<WorkspaceDb>>>,
        inbox_db: Rc<RefCell<Option<InboxDb>>>,
        known_ws: Rc<RefCell<KnownWorkspaceRegistry>>,
        on_return_to_editor: impl Fn() + 'static,
        on_open_inbox_note: impl Fn(String) + 'static,
        on_new_inbox_note: impl Fn() + 'static,
        on_place_inbox_note: impl Fn(String, String) + 'static,
        on_open_workspace: impl Fn(std::path::PathBuf) + 'static,
        on_new_workspace: impl Fn() + 'static,
        on_open_workspace_note: impl Fn(String) + 'static,
        on_new_workspace_note: impl Fn() + 'static,
    ) -> Self {
        // Wrap callbacks in Rc so they can be cheaply shared across closures.
        let on_return_to_editor: Rc<dyn Fn()> = Rc::new(on_return_to_editor);
        let on_open_inbox_note: Rc<dyn Fn(String)> = Rc::new(on_open_inbox_note);
        let on_new_inbox_note: Rc<dyn Fn()> = Rc::new(on_new_inbox_note);
        let on_place_inbox_note: Rc<dyn Fn(String, String)> = Rc::new(on_place_inbox_note);
        let on_open_workspace: Rc<dyn Fn(std::path::PathBuf)> = Rc::new(on_open_workspace);
        let on_new_workspace: Rc<dyn Fn()> = Rc::new(on_new_workspace);
        let on_open_workspace_note: Rc<dyn Fn(String)> = Rc::new(on_open_workspace_note);
        let on_new_workspace_note: Rc<dyn Fn()> = Rc::new(on_new_workspace_note);

        // ── Root: horizontal three-panel layout ───────────────────────────────
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("desk-shell");
        root.set_hexpand(true);
        root.set_vexpand(true);

        // ══ LEFT PANEL ════════════════════════════════════════════════════════
        let left_panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
        left_panel.add_css_class("desk-left-panel");
        left_panel.set_size_request(260, -1);

        let left_scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        left_scroll.add_css_class("desk-scroll");

        let left_inner = gtk::Box::new(gtk::Orientation::Vertical, 0);
        left_inner.set_margin_bottom(16);

        // ── Return-to-editor button ────────────────────────────────────────
        let return_btn = gtk::Button::with_label("← Return to Editor");
        return_btn.add_css_class("desk-return-btn");
        return_btn.set_margin_start(12);
        return_btn.set_margin_end(12);
        return_btn.set_margin_top(12);
        return_btn.set_margin_bottom(8);
        return_btn.set_tooltip_text(Some("Return to the note editor (Escape)"));

        left_inner.append(&return_btn);
        left_inner.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

        // ── Inbox section ──────────────────────────────────────────────────
        let (inbox_hdr, inbox_count_label) =
            make_section_header("Inbox", "New Note", "Create a new Inbox note");
        let inbox_list = gtk::ListBox::new();
        inbox_list.add_css_class("desk-note-list");
        inbox_list.set_selection_mode(gtk::SelectionMode::Single);
        inbox_list.set_margin_start(8);
        inbox_list.set_margin_end(8);
        inbox_list.set_margin_bottom(4);
        let inbox_empty_label =
            make_empty_label("Nothing in your Inbox.\nNotes you capture quickly appear here.");

        left_inner.append(&inbox_hdr);
        left_inner.append(&inbox_empty_label);
        left_inner.append(&inbox_list);
        left_inner.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

        // ── Pinned section ─────────────────────────────────────────────────
        let (pins_hdr, _) =
            make_section_header_no_action("Pinned", "★ Pinned notes across all workspaces");
        let pins_list = gtk::ListBox::new();
        pins_list.add_css_class("desk-note-list");
        pins_list.set_selection_mode(gtk::SelectionMode::Single);
        pins_list.set_margin_start(8);
        pins_list.set_margin_end(8);
        pins_list.set_margin_bottom(4);
        let pins_empty_label =
            make_empty_label("No pinned notes yet.\nPin a note to keep it here.");

        left_inner.append(&pins_hdr);
        left_inner.append(&pins_empty_label);
        left_inner.append(&pins_list);
        left_inner.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

        // ── Recent section ─────────────────────────────────────────────────
        let (recent_hdr, _) = make_section_header_no_action("Recent", "Recently opened notes");
        let recent_list = gtk::ListBox::new();
        recent_list.add_css_class("desk-note-list");
        recent_list.set_selection_mode(gtk::SelectionMode::Single);
        recent_list.set_margin_start(8);
        recent_list.set_margin_end(8);
        recent_list.set_margin_bottom(4);
        let recent_empty_label = make_empty_label("No recently opened notes.");

        left_inner.append(&recent_hdr);
        left_inner.append(&recent_empty_label);
        left_inner.append(&recent_list);
        left_inner.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

        // ── Workspaces sidebar section ─────────────────────────────────────
        let ws_sidebar_hdr_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        ws_sidebar_hdr_box.add_css_class("desk-section-header");
        ws_sidebar_hdr_box.set_margin_start(16);
        ws_sidebar_hdr_box.set_margin_end(12);
        ws_sidebar_hdr_box.set_margin_top(10);
        ws_sidebar_hdr_box.set_margin_bottom(4);

        let ws_sidebar_title = gtk::Label::new(Some("Workspaces"));
        ws_sidebar_title.add_css_class("desk-section-title");
        ws_sidebar_title.set_halign(gtk::Align::Start);
        ws_sidebar_title.set_hexpand(true);
        ws_sidebar_hdr_box.append(&ws_sidebar_title);

        let ws_sidebar_list = gtk::ListBox::new();
        ws_sidebar_list.add_css_class("desk-note-list");
        ws_sidebar_list.set_selection_mode(gtk::SelectionMode::Single);
        ws_sidebar_list.set_margin_start(8);
        ws_sidebar_list.set_margin_end(8);
        ws_sidebar_list.set_margin_bottom(4);
        let ws_sidebar_empty_label =
            make_empty_label("No workspaces yet.\nCreate or open a .water file.");

        let ws_actions_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        ws_actions_box.set_margin_start(16);
        ws_actions_box.set_margin_end(12);
        ws_actions_box.set_margin_top(4);
        ws_actions_box.set_margin_bottom(8);
        let open_ws_btn = gtk::Button::with_label("Open…");
        open_ws_btn.add_css_class("desk-new-btn");
        open_ws_btn.set_tooltip_text(Some("Open an existing .water workspace file"));
        let new_ws_btn = gtk::Button::with_label("New Workspace");
        new_ws_btn.add_css_class("desk-new-btn");
        new_ws_btn.set_tooltip_text(Some("Create a new .water workspace"));
        ws_actions_box.append(&open_ws_btn);
        ws_actions_box.append(&new_ws_btn);

        left_inner.append(&ws_sidebar_hdr_box);
        left_inner.append(&ws_sidebar_empty_label);
        left_inner.append(&ws_sidebar_list);
        left_inner.append(&ws_actions_box);

        left_scroll.set_child(Some(&left_inner));
        left_panel.append(&left_scroll);
        root.append(&left_panel);
        root.append(&gtk::Separator::new(gtk::Orientation::Vertical));

        // ══ CENTER PANEL ══════════════════════════════════════════════════════
        let center_panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
        center_panel.set_hexpand(true);
        center_panel.set_vexpand(true);

        let center_scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        center_scroll.add_css_class("desk-scroll");

        let center_content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        center_content.set_margin_top(16);
        center_content.set_margin_bottom(24);
        center_content.set_margin_start(20);
        center_content.set_margin_end(20);

        center_scroll.set_child(Some(&center_content));
        center_panel.append(&center_scroll);
        root.append(&center_panel);
        root.append(&gtk::Separator::new(gtk::Orientation::Vertical));

        // ══ RIGHT PANEL ═══════════════════════════════════════════════════════
        let right_panel = build_right_panel(
            on_new_inbox_note.clone(),
            on_new_workspace_note.clone(),
            workspace_db.clone(),
        );
        root.append(&right_panel);

        // ── Build struct ──────────────────────────────────────────────────────
        let shell = DeskShell {
            root,
            inbox_list: inbox_list.clone(),
            inbox_count_label: inbox_count_label.clone(),
            inbox_empty_label: inbox_empty_label.clone(),
            pins_list: pins_list.clone(),
            pins_empty_label: pins_empty_label.clone(),
            recent_list: recent_list.clone(),
            recent_empty_label: recent_empty_label.clone(),
            ws_sidebar_list: ws_sidebar_list.clone(),
            ws_sidebar_empty_label: ws_sidebar_empty_label.clone(),
            center_content: center_content.clone(),
            workspace_db: workspace_db.clone(),
            inbox_db: inbox_db.clone(),
            known_ws: known_ws.clone(),
            on_open_inbox_note: on_open_inbox_note.clone(),
            on_place_inbox_note: on_place_inbox_note.clone(),
            on_open_workspace: on_open_workspace.clone(),
            on_new_workspace: on_new_workspace.clone(),
            on_open_workspace_note: on_open_workspace_note.clone(),
            on_new_workspace_note: on_new_workspace_note.clone(),
        };

        // ── Wire static connections (connected once) ───────────────────────────

        // Return to editor
        {
            let cb = on_return_to_editor.clone();
            return_btn.connect_clicked(move |_| (cb)());
        }

        // Inbox note row activation
        {
            let shell2 = shell.clone();
            inbox_list.connect_row_activated(move |_, row| {
                let note_id = row.widget_name().to_string();
                if note_id.is_empty() {
                    return;
                }
                // Track recent
                shell2.touch_recent_inbox_note(&note_id);
                (shell2.on_open_inbox_note)(note_id);
            });
        }

        // Inbox section "New Note" button (extracted from header)
        // — wired via on_new_inbox_note captured in header already
        // The header button's signal is connected below via the returned widget handle.
        {
            // The header button is stored in inbox_hdr. Extract it by traversal.
            let new_btn = find_button_in_box(&inbox_hdr);
            if let Some(btn) = new_btn {
                let cb = on_new_inbox_note.clone();
                btn.connect_clicked(move |_| (cb)());
            }
        }

        // Pins row activation
        {
            let shell2 = shell.clone();
            pins_list.connect_row_activated(move |_, row| {
                let key = row.widget_name().to_string();
                if key.is_empty() {
                    return;
                }
                // key format: "inbox_note::<note_id>" or "workspace_note::<ws_path>::<note_id>"
                shell2.activate_pin_row(&key);
            });
        }

        // Recent row activation
        {
            let shell2 = shell.clone();
            recent_list.connect_row_activated(move |_, row| {
                let key = row.widget_name().to_string();
                if key.is_empty() {
                    return;
                }
                shell2.activate_recent_row(&key);
            });
        }

        // Workspace sidebar row activation
        {
            let on_open_ws = on_open_workspace.clone();
            ws_sidebar_list.connect_row_activated(move |_, row| {
                let path_str = row.widget_name().to_string();
                if !path_str.is_empty() {
                    (on_open_ws)(std::path::PathBuf::from(path_str));
                }
            });
        }

        // Open Workspace file-chooser button
        {
            let cb = on_open_workspace.clone();
            open_ws_btn.connect_clicked(move |btn| {
                show_open_workspace_dialog(btn, cb.clone());
            });
        }

        // New Workspace button
        {
            let cb = on_new_workspace.clone();
            new_ws_btn.connect_clicked(move |_| (cb)());
        }

        shell
    }

    // ── Public refresh API ────────────────────────────────────────────────────

    /// Refresh all Desk sections. Call each time Desk becomes the visible mode.
    pub fn refresh(&self) {
        self.refresh_inbox();
        self.refresh_pins();
        self.refresh_recent();
        self.refresh_workspace_sidebar();
        self.refresh_center();
    }

    // ── Private refresh methods ───────────────────────────────────────────────

    fn refresh_inbox(&self) {
        clear_list(&self.inbox_list);

        let notes = {
            let guard = self.inbox_db.borrow();
            guard
                .as_ref()
                .and_then(|db| db.list_notes().ok())
                .unwrap_or_default()
        };

        self.inbox_empty_label.set_visible(notes.is_empty());
        self.inbox_count_label
            .set_text(&format!("Inbox ({})", notes.len()));

        for note in &notes {
            let row = gtk::ListBoxRow::new();
            row.add_css_class("desk-note-row");
            row.set_widget_name(&note.id);

            let row_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
            row_box.set_margin_start(12);
            row_box.set_margin_end(12);
            row_box.set_margin_top(7);
            row_box.set_margin_bottom(7);

            let meta = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            let title_lbl = make_label(&note.title, "desk-note-title", gtk::Align::Start, true);
            let date_lbl = make_label(
                format_date_short(&note.updated_at),
                "desk-note-date",
                gtk::Align::End,
                false,
            );
            meta.append(&title_lbl);
            meta.append(&date_lbl);

            let snippet = first_line_preview(&note.body, 72);
            let preview_lbl = make_label(&snippet, "desk-note-preview", gtk::Align::Start, false);

            row_box.append(&meta);
            row_box.append(&preview_lbl);

            // Bottom action row: pin indicator + Place button
            let action_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);

            if note.is_pinned {
                let pin_lbl =
                    make_label("★ Pinned", "desk-pin-indicator", gtk::Align::Start, false);
                pin_lbl.set_hexpand(true);
                action_row.append(&pin_lbl);
            } else {
                let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                spacer.set_hexpand(true);
                action_row.append(&spacer);
            }

            let place_btn = gtk::Button::with_label("Place…");
            place_btn.add_css_class("desk-card-place-btn");
            place_btn.set_tooltip_text(Some("Move this note into a workspace"));
            {
                let cb = self.on_place_inbox_note.clone();
                let note_id = note.id.clone();
                let note_title = note.title.clone();
                place_btn.connect_clicked(move |_| (cb)(note_id.clone(), note_title.clone()));
            }
            action_row.append(&place_btn);

            row_box.append(&action_row);
            row.set_child(Some(&row_box));
            self.inbox_list.append(&row);
        }
    }

    fn refresh_pins(&self) {
        clear_list(&self.pins_list);

        let pins = {
            let guard = self.inbox_db.borrow();
            guard
                .as_ref()
                .and_then(|db| db.list_pins().ok())
                .unwrap_or_default()
        };

        self.pins_empty_label.set_visible(pins.is_empty());

        for pin in &pins {
            let row = gtk::ListBoxRow::new();
            row.add_css_class("desk-note-row");
            // Encode enough info into the widget name for activation.
            let key = if pin.target_kind == "inbox_note" {
                format!("inbox_note::{}", pin.target_id)
            } else {
                format!("workspace_note::{}::{}", pin.workspace_path, pin.target_id)
            };
            row.set_widget_name(&key);
            row.set_tooltip_text(Some(&format!(
                "Pinned {} · created {} · order {}",
                pin.id,
                format_date_short(&pin.created_at),
                pin.sort_order
            )));

            let row_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
            row_box.set_margin_start(12);
            row_box.set_margin_end(12);
            row_box.set_margin_top(7);
            row_box.set_margin_bottom(7);

            let meta = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            let display_title = if pin.note_title.is_empty() {
                "Untitled note".to_string()
            } else {
                pin.note_title.clone()
            };
            let title_lbl = make_label(&display_title, "desk-note-title", gtk::Align::Start, true);
            let pin_icon = make_label("★", "desk-pin-icon", gtk::Align::End, false);
            meta.append(&title_lbl);
            meta.append(&pin_icon);
            row_box.append(&meta);

            let location = pin_location_label(pin);
            let loc_lbl = make_label(&location, "desk-note-preview", gtk::Align::Start, false);
            row_box.append(&loc_lbl);

            if !pin.note_snippet.trim().is_empty() {
                let snippet_lbl = make_label(
                    &pin.note_snippet,
                    "desk-note-preview",
                    gtk::Align::Start,
                    false,
                );
                row_box.append(&snippet_lbl);
            }

            row.set_child(Some(&row_box));
            self.pins_list.append(&row);
        }
    }

    fn refresh_recent(&self) {
        clear_list(&self.recent_list);

        let entries = {
            let guard = self.inbox_db.borrow();
            guard
                .as_ref()
                .and_then(|db| db.list_recent(15).ok())
                .unwrap_or_default()
        };

        self.recent_empty_label.set_visible(entries.is_empty());

        for entry in &entries {
            let row = gtk::ListBoxRow::new();
            row.add_css_class("desk-note-row");
            let key = if entry.target_kind == "inbox_note" {
                format!("inbox_note::{}", entry.target_id)
            } else {
                format!(
                    "workspace_note::{}::{}",
                    entry.workspace_path, entry.target_id
                )
            };
            row.set_widget_name(&key);

            let row_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
            row_box.set_margin_start(12);
            row_box.set_margin_end(12);
            row_box.set_margin_top(7);
            row_box.set_margin_bottom(7);

            let meta = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            let display_title = if entry.note_title.is_empty() {
                "Untitled note".to_string()
            } else {
                entry.note_title.clone()
            };
            let title_lbl = make_label(&display_title, "desk-note-title", gtk::Align::Start, true);
            let date_lbl = make_label(
                format_date_short(&entry.accessed_at),
                "desk-note-date",
                gtk::Align::End,
                false,
            );
            meta.append(&title_lbl);
            meta.append(&date_lbl);
            row_box.append(&meta);

            let location = recent_location_label(entry);
            let loc_lbl = make_label(&location, "desk-note-preview", gtk::Align::Start, false);
            row_box.append(&loc_lbl);

            row.set_child(Some(&row_box));
            self.recent_list.append(&row);
        }
    }

    fn refresh_workspace_sidebar(&self) {
        clear_list(&self.ws_sidebar_list);

        let workspaces = self.known_ws.borrow().list().to_vec();
        self.ws_sidebar_empty_label
            .set_visible(workspaces.is_empty());

        let focused_path: Option<String> = self
            .workspace_db
            .borrow()
            .as_ref()
            .map(|db| db.path.to_string_lossy().into_owned());

        for ws in &workspaces {
            let row = gtk::ListBoxRow::new();
            row.add_css_class("desk-note-row");
            let path_str = ws.path.to_string_lossy().into_owned();
            row.set_widget_name(&path_str);

            let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            row_box.set_margin_start(12);
            row_box.set_margin_end(12);
            row_box.set_margin_top(6);
            row_box.set_margin_bottom(6);

            let name_lbl = make_label(&ws.display_name, "desk-note-title", gtk::Align::Start, true);

            if focused_path.as_deref() == Some(&path_str) {
                let active_lbl = make_label("✓", "desk-ws-active", gtk::Align::End, false);
                row_box.append(&name_lbl);
                row_box.append(&active_lbl);
            } else {
                row_box.append(&name_lbl);
            }

            row.set_child(Some(&row_box));
            self.ws_sidebar_list.append(&row);
        }
    }

    fn refresh_center(&self) {
        clear_box(&self.center_content);

        // Collect workspace data without holding the borrow during UI build.
        let snap: Option<WsSnap> = {
            let guard = self.workspace_db.borrow();
            guard.as_ref().map(|db| collect_workspace_snapshot(db))
        };

        let Some(snap) = snap else {
            // No workspace focused — show empty state.
            self.center_content.append(&build_center_empty_state(
                self.on_open_workspace.clone(),
                self.on_new_workspace.clone(),
            ));
            return;
        };

        // Workspace header row
        let ws_header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        ws_header.set_margin_bottom(8);

        let ws_name_lbl = gtk::Label::new(Some(&snap.name));
        ws_name_lbl.add_css_class("desk-ws-name");
        ws_name_lbl.set_halign(gtk::Align::Start);
        ws_name_lbl.set_hexpand(true);

        let new_note_btn = gtk::Button::with_label("+ New Note");
        new_note_btn.add_css_class("desk-new-btn");
        new_note_btn.set_tooltip_text(Some("Create a new note in this workspace (Ctrl+Shift+N)"));
        {
            let cb = self.on_new_workspace_note.clone();
            new_note_btn.connect_clicked(move |_| (cb)());
        }

        ws_header.append(&ws_name_lbl);
        ws_header.append(&new_note_btn);
        self.center_content.append(&ws_header);

        self.center_content
            .append(&gtk::Separator::new(gtk::Orientation::Horizontal));

        // Room sections
        if snap.rooms.is_empty() {
            let lbl = make_label(
                "No rooms yet. Create a room to organize your notes.",
                "desk-empty-label",
                gtk::Align::Start,
                false,
            );
            lbl.set_margin_top(16);
            self.center_content.append(&lbl);
        } else {
            for room in &snap.rooms {
                let room_section = self.build_room_section(room, &snap.path, &snap.name);
                self.center_content.append(&room_section);
            }
        }
    }

    fn build_room_section(&self, room: &RoomSnap, ws_path: &str, ws_name: &str) -> gtk::Box {
        let section = gtk::Box::new(gtk::Orientation::Vertical, 0);
        section.add_css_class("desk-room-section");
        section.set_margin_top(16);

        // Room header
        let room_hdr = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        room_hdr.add_css_class("desk-room-header");
        room_hdr.set_margin_bottom(6);

        let room_name_lbl = gtk::Label::new(Some(&format!("Room: {}", room.name)));
        room_name_lbl.add_css_class("desk-room-name");
        room_name_lbl.set_halign(gtk::Align::Start);
        room_name_lbl.set_hexpand(true);

        room_hdr.append(&room_name_lbl);

        // Action buttons (wired via workspace_db for create operations)
        for (label, kind, tip) in &[
            (
                "+ Shelf",
                ContainerKind::Shelf,
                "Create a new Shelf in this Room",
            ),
            (
                "+ Pile",
                ContainerKind::Pile,
                "Create a new Pile in this Room",
            ),
        ] {
            let btn = gtk::Button::with_label(label);
            btn.add_css_class("desk-action-btn");
            btn.set_tooltip_text(Some(tip));
            let shell = self.clone();
            let room_id = room.id.clone();
            let kind = kind.clone();
            btn.connect_clicked(move |btn| {
                shell.prompt_create_container_in_room(btn, &room_id, kind.clone());
            });
            room_hdr.append(&btn);
        }

        section.append(&room_hdr);

        // Shelves
        for shelf in &room.shelves {
            let container_box =
                self.build_container_section(shelf, &room.id, &room.name, ws_path, ws_name);
            section.append(&container_box);
        }

        // Piles
        for pile in &room.piles {
            let container_box =
                self.build_container_section(pile, &room.id, &room.name, ws_path, ws_name);
            section.append(&container_box);
        }

        // Loose Notes
        let loose_hdr = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        loose_hdr.set_margin_top(8);
        loose_hdr.set_margin_bottom(4);

        let loose_lbl = gtk::Label::new(Some(&format!("Loose Notes ({})", room.loose_count)));
        loose_lbl.add_css_class("desk-container-label");
        loose_lbl.add_css_class("desk-loose-label");
        loose_lbl.set_halign(gtk::Align::Start);
        loose_lbl.set_hexpand(true);

        let new_loose_btn = gtk::Button::with_label("+ New");
        new_loose_btn.add_css_class("desk-action-btn");
        new_loose_btn.set_tooltip_text(Some("Create a new loose note in this Room"));
        {
            let cb = self.on_new_workspace_note.clone();
            new_loose_btn.connect_clicked(move |_| (cb)());
        }
        loose_hdr.append(&loose_lbl);
        loose_hdr.append(&new_loose_btn);
        section.append(&loose_hdr);

        if room.loose_notes.is_empty() {
            let empty = make_label(
                "No loose notes in this Room.",
                "desk-container-empty",
                gtk::Align::Start,
                false,
            );
            empty.set_margin_start(8);
            empty.set_margin_bottom(4);
            section.append(&empty);
        } else {
            for note in &room.loose_notes {
                let card = self.build_note_card(
                    note,
                    ws_path,
                    ws_name,
                    &format_location_label(false, Some(&room.name), None, false),
                    "workspace_note",
                );
                section.append(&card);
            }
        }

        section
    }

    fn build_container_section(
        &self,
        container: &ContainerSnap,
        room_id: &str,
        room_name: &str,
        ws_path: &str,
        ws_name: &str,
    ) -> gtk::Box {
        let box_ = gtk::Box::new(gtk::Orientation::Vertical, 0);
        box_.set_margin_top(6);
        let is_pile = container.is_pile;

        let hdr = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        hdr.set_margin_bottom(4);

        let icon = if is_pile { "📦" } else { "📚" };
        let hdr_text = format!(
            "{icon} {} ({}) · {}",
            container.name,
            container.note_count,
            if is_pile { "Pile" } else { "Shelf" }
        );
        let lbl = gtk::Label::new(Some(&hdr_text));
        lbl.add_css_class("desk-container-label");
        lbl.set_halign(gtk::Align::Start);
        lbl.set_hexpand(true);
        hdr.append(&lbl);

        if is_pile {
            let convert_btn = gtk::Button::with_label("→ Shelf");
            convert_btn.add_css_class("desk-action-btn");
            convert_btn.set_tooltip_text(Some(&format!(
                "Convert '{}' from Pile to Shelf",
                container.name
            )));
            let shell = self.clone();
            let cid = container.id.clone();
            let rid = room_id.to_string();
            convert_btn.connect_clicked(move |_| {
                shell.convert_pile_to_shelf_action(&cid, &rid);
            });
            hdr.append(&convert_btn);
        }

        box_.append(&hdr);

        if container.notes.is_empty() {
            let empty = make_label(
                if is_pile {
                    "Pile is empty."
                } else {
                    "Shelf is empty."
                },
                "desk-container-empty",
                gtk::Align::Start,
                false,
            );
            empty.set_margin_start(8);
            empty.set_margin_bottom(4);
            box_.append(&empty);
        } else {
            let location_label =
                format_location_label(false, Some(room_name), Some(&container.name), is_pile);
            for note in &container.notes {
                let card =
                    self.build_note_card(note, ws_path, ws_name, &location_label, "workspace_note");
                box_.append(&card);
            }
        }

        box_
    }

    /// Build a note card widget for the center panel.
    fn build_note_card(
        &self,
        note: &NoteSnap,
        ws_path: &str,
        ws_name: &str,
        location: &str,
        target_kind: &str,
    ) -> gtk::Box {
        let card = gtk::Box::new(gtk::Orientation::Vertical, 2);
        card.add_css_class("desk-note-card");
        card.set_margin_start(8);
        card.set_margin_bottom(4);

        // Top row: title + date + pin button
        let top = gtk::Box::new(gtk::Orientation::Horizontal, 4);

        let title_display = if note.title.is_empty() {
            "Untitled note".to_string()
        } else {
            note.title.clone()
        };
        let title_lbl = make_label(&title_display, "desk-card-title", gtk::Align::Start, true);
        let date_lbl = make_label(
            format_date_short(&note.updated_at),
            "desk-card-date",
            gtk::Align::End,
            false,
        );

        // Check pin state (borrow and release immediately)
        let is_pinned = {
            let guard = self.inbox_db.borrow();
            guard
                .as_ref()
                .map(|db| db.is_note_pinned(target_kind, &note.id, ws_path))
                .unwrap_or(false)
        };

        let pin_btn = gtk::Button::with_label(if is_pinned { "★" } else { "☆" });
        pin_btn.add_css_class("desk-card-pin-btn");
        pin_btn.set_tooltip_text(Some(if is_pinned {
            "Unpin this note"
        } else {
            "Pin this note globally"
        }));

        {
            let shell = self.clone();
            let note_id = note.id.clone();
            let note_title = title_display.clone();
            let note_snippet = note.snippet.clone();
            let ws_path = ws_path.to_string();
            let target_kind = target_kind.to_string();
            pin_btn.connect_clicked(move |_| {
                shell.toggle_pin(&target_kind, &note_id, &ws_path, &note_title, &note_snippet);
            });
        }

        top.append(&title_lbl);
        top.append(&date_lbl);
        top.append(&pin_btn);
        card.append(&top);

        // Bottom row: snippet + location + open button
        let bottom = gtk::Box::new(gtk::Orientation::Horizontal, 4);

        let snippet_lbl = make_label(&note.snippet, "desk-card-snippet", gtk::Align::Start, true);

        let loc_lbl = make_label(location, "desk-card-location", gtk::Align::End, false);

        let open_btn = gtk::Button::with_label("Open");
        open_btn.add_css_class("desk-card-open-btn");
        open_btn.set_tooltip_text(Some("Open this note in the editor"));

        {
            let shell = self.clone();
            let note_id = note.id.clone();
            let note_title = title_display.clone();
            let note_snippet = note.snippet.clone();
            let ws_path_owned = ws_path.to_string();
            let ws_name_owned = ws_name.to_string();
            let target_kind = target_kind.to_string();
            let cb = self.on_open_workspace_note.clone();
            open_btn.connect_clicked(move |_| {
                // Track recent access
                {
                    let guard = shell.inbox_db.borrow();
                    if let Some(db) = guard.as_ref() {
                        let entry = RecentEntry {
                            id: new_note_id(),
                            target_kind: target_kind.clone(),
                            target_id: note_id.clone(),
                            workspace_path: ws_path_owned.clone(),
                            workspace_name: ws_name_owned.clone(),
                            note_title: note_title.clone(),
                            note_snippet: note_snippet.clone(),
                            accessed_at: now_iso8601(),
                        };
                        let _ = db.touch_recent(&entry);
                    }
                }
                (cb)(note_id.clone());
            });
        }

        bottom.append(&snippet_lbl);
        bottom.append(&loc_lbl);
        bottom.append(&open_btn);
        card.append(&bottom);

        // Placeholder chips for future Search Mode (Prompt 6)
        let chips_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        chips_row.add_css_class("desk-card-chips");
        // palette_tint placeholder, bookmark placeholder, linked-object icons placeholder
        card.append(&chips_row);

        card
    }

    // ── Pin / Recent helpers ──────────────────────────────────────────────────

    fn toggle_pin(
        &self,
        target_kind: &str,
        note_id: &str,
        ws_path: &str,
        note_title: &str,
        note_snippet: &str,
    ) {
        let guard = self.inbox_db.borrow();
        let Some(db) = guard.as_ref() else { return };

        if db.is_note_pinned(target_kind, note_id, ws_path) {
            let _ = db.unpin_note(target_kind, note_id, ws_path);
        } else {
            let _ = db.pin_note(target_kind, note_id, ws_path, note_title, note_snippet);
        }
        drop(guard);
        // Rebuild pins section and center (pin buttons need to refresh)
        self.refresh_pins();
        self.refresh_center();
    }

    fn touch_recent_inbox_note(&self, note_id: &str) {
        let guard = self.inbox_db.borrow();
        let Some(db) = guard.as_ref() else { return };
        // Fetch the note to get current title/snippet.
        if let Ok(Some(note)) = db.get_note(note_id) {
            let snippet = first_line_preview(&note.body, 72);
            let entry = RecentEntry {
                id: new_note_id(),
                target_kind: "inbox_note".to_string(),
                target_id: note_id.to_string(),
                workspace_path: "".to_string(),
                workspace_name: "Inbox".to_string(),
                note_title: note.title.clone(),
                note_snippet: snippet,
                accessed_at: now_iso8601(),
            };
            let _ = db.touch_recent(&entry);
        }
    }

    fn activate_pin_row(&self, key: &str) {
        if let Some(note_id) = key.strip_prefix("inbox_note::") {
            self.touch_recent_inbox_note(note_id);
            (self.on_open_inbox_note)(note_id.to_string());
        } else if let Some(rest) = key.strip_prefix("workspace_note::") {
            // Format: ws_path::note_id (note_id has no '::')
            if let Some((ws_path, note_id)) = rest.rsplit_once("::") {
                // Check if this workspace is the focused one; if so open directly.
                let focused_path = self
                    .workspace_db
                    .borrow()
                    .as_ref()
                    .map(|db| db.path.to_string_lossy().into_owned());
                if focused_path.as_deref() == Some(ws_path) {
                    (self.on_open_workspace_note)(note_id.to_string());
                } else if !ws_path.is_empty() {
                    // Open the workspace then the note.
                    let path = std::path::PathBuf::from(ws_path);
                    if path.exists() {
                        (self.on_open_workspace)(path);
                        (self.on_open_workspace_note)(note_id.to_string());
                    } else {
                        eprintln!("blot: pinned workspace not found: {ws_path}");
                    }
                }
            }
        }
    }

    fn activate_recent_row(&self, key: &str) {
        if let Some(note_id) = key.strip_prefix("inbox_note::") {
            self.touch_recent_inbox_note(note_id);
            (self.on_open_inbox_note)(note_id.to_string());
        } else if let Some(rest) = key.strip_prefix("workspace_note::") {
            if let Some((ws_path, note_id)) = rest.rsplit_once("::") {
                let focused_path = self
                    .workspace_db
                    .borrow()
                    .as_ref()
                    .map(|db| db.path.to_string_lossy().into_owned());
                if focused_path.as_deref() == Some(ws_path) {
                    (self.on_open_workspace_note)(note_id.to_string());
                } else if !ws_path.is_empty() {
                    let path = std::path::PathBuf::from(ws_path);
                    if path.exists() {
                        (self.on_open_workspace)(path);
                        (self.on_open_workspace_note)(note_id.to_string());
                    }
                }
            }
        }
    }

    // ── Workspace actions (invoked from center panel) ─────────────────────────

    fn convert_pile_to_shelf_action(&self, pile_id: &str, room_id: &str) {
        let result = self
            .workspace_db
            .borrow()
            .as_ref()
            .map(|db| db.convert_pile_to_shelf(pile_id));
        match result {
            Some(Ok(())) => self.refresh_center(),
            Some(Err(e)) => eprintln!("blot: desk convert pile: {e}"),
            None => {}
        }
        let _ = room_id; // room_id not needed for convert, just for future rename refresh
    }

    fn prompt_create_container_in_room(
        &self,
        parent: &gtk::Button,
        room_id: &str,
        kind: ContainerKind,
    ) {
        let toplevel = parent
            .ancestor(gtk::Window::static_type())
            .and_then(|w| w.downcast::<gtk::Window>().ok());

        let shell = self.clone();
        let rid = room_id.to_string();
        show_text_prompt(
            toplevel.as_ref(),
            &format!("New {}", kind.display_label()),
            Some(&format!("{} name", kind.display_label())),
            None,
            "Create",
            move |name| {
                let result = shell
                    .workspace_db
                    .borrow()
                    .as_ref()
                    .map(|db| db.create_container(&rid, &name, kind.clone()));
                if let Some(Ok(_)) = result {
                    shell.refresh_center();
                }
            },
        );
    }
}

// ── Widget builder helpers ─────────────────────────────────────────────────────

fn make_label(text: &str, css: &str, align: gtk::Align, expand: bool) -> gtk::Label {
    let lbl = gtk::Label::new(Some(text));
    lbl.add_css_class(css);
    lbl.set_halign(align);
    lbl.set_hexpand(expand);
    lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
    lbl
}

fn make_empty_label(text: &str) -> gtk::Label {
    let lbl = gtk::Label::new(Some(text));
    lbl.add_css_class("desk-empty-label");
    lbl.set_justify(gtk::Justification::Center);
    lbl.set_margin_top(8);
    lbl.set_margin_bottom(8);
    lbl.set_margin_start(16);
    lbl.set_margin_end(16);
    lbl.set_visible(false);
    lbl
}

/// Build a section header with an action button, returning (header_box, section_title_label).
fn make_section_header(
    title: &str,
    action_label: &str,
    action_tip: &str,
) -> (gtk::Box, gtk::Label) {
    let hdr = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    hdr.add_css_class("desk-section-header");
    hdr.set_margin_start(16);
    hdr.set_margin_end(12);
    hdr.set_margin_top(10);
    hdr.set_margin_bottom(4);

    let title_lbl = gtk::Label::new(Some(title));
    title_lbl.add_css_class("desk-section-title");
    title_lbl.set_halign(gtk::Align::Start);
    title_lbl.set_hexpand(true);

    let action_btn = gtk::Button::with_label(action_label);
    action_btn.add_css_class("desk-new-btn");
    action_btn.set_tooltip_text(Some(action_tip));

    hdr.append(&title_lbl);
    hdr.append(&action_btn);
    (hdr, title_lbl)
}

/// Build a section header with no action button.
fn make_section_header_no_action(title: &str, _tooltip: &str) -> (gtk::Box, gtk::Label) {
    let hdr = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    hdr.add_css_class("desk-section-header");
    hdr.set_margin_start(16);
    hdr.set_margin_end(12);
    hdr.set_margin_top(10);
    hdr.set_margin_bottom(4);

    let title_lbl = gtk::Label::new(Some(title));
    title_lbl.add_css_class("desk-section-title");
    title_lbl.set_halign(gtk::Align::Start);
    title_lbl.set_hexpand(true);
    hdr.append(&title_lbl);
    (hdr, title_lbl)
}

fn clear_list(list: &gtk::ListBox) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
}

fn clear_box(box_: &gtk::Box) {
    while let Some(child) = box_.first_child() {
        box_.remove(&child);
    }
}

/// Find the first Button child in a Box (used to wire the section-header action button).
fn find_button_in_box(box_: &gtk::Box) -> Option<gtk::Button> {
    let mut child = box_.first_child();
    while let Some(w) = child {
        if let Ok(btn) = w.clone().downcast::<gtk::Button>() {
            return Some(btn);
        }
        child = w.next_sibling();
    }
    None
}

// ── Right panel builder ────────────────────────────────────────────────────────

fn build_right_panel(
    on_new_inbox_note: Rc<dyn Fn()>,
    on_new_workspace_note: Rc<dyn Fn()>,
    workspace_db: Rc<RefCell<Option<WorkspaceDb>>>,
) -> gtk::Box {
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
    panel.add_css_class("desk-right-panel");
    panel.set_size_request(200, -1);

    let inner = gtk::Box::new(gtk::Orientation::Vertical, 0);
    inner.set_margin_top(16);
    inner.set_margin_start(12);
    inner.set_margin_end(12);

    let actions_lbl = gtk::Label::new(Some("Quick Actions"));
    actions_lbl.add_css_class("desk-right-section-title");
    actions_lbl.set_halign(gtk::Align::Start);
    actions_lbl.set_margin_bottom(8);
    inner.append(&actions_lbl);

    let new_inbox_btn = gtk::Button::with_label("New Inbox Note");
    new_inbox_btn.add_css_class("desk-right-btn");
    new_inbox_btn.set_tooltip_text(Some("Create a new note in your Inbox (Ctrl+N)"));
    {
        let cb = on_new_inbox_note.clone();
        new_inbox_btn.connect_clicked(move |_| (cb)());
    }
    inner.append(&new_inbox_btn);

    let new_ws_note_btn = gtk::Button::with_label("New Workspace Note");
    new_ws_note_btn.add_css_class("desk-right-btn");
    new_ws_note_btn.set_tooltip_text(Some(
        "Create a new note in the focused workspace (Ctrl+Shift+N)",
    ));
    {
        let cb = on_new_workspace_note.clone();
        let ws_db = workspace_db.clone();
        new_ws_note_btn.connect_clicked(move |btn| {
            if ws_db.borrow().is_some() {
                (cb)();
            } else {
                btn.set_tooltip_text(Some("No workspace open — open or create one first"));
            }
        });
    }
    inner.append(&new_ws_note_btn);

    inner.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    let ws_section_lbl = gtk::Label::new(Some("Workspace"));
    ws_section_lbl.add_css_class("desk-right-section-title");
    ws_section_lbl.set_halign(gtk::Align::Start);
    ws_section_lbl.set_margin_top(12);
    ws_section_lbl.set_margin_bottom(8);
    inner.append(&ws_section_lbl);

    let ws_status_lbl = {
        let ws_name = workspace_db
            .borrow()
            .as_ref()
            .map(|db| db.workspace_name())
            .unwrap_or_else(|| "No workspace open".to_string());
        gtk::Label::new(Some(&ws_name))
    };
    ws_status_lbl.add_css_class("desk-right-ws-name");
    ws_status_lbl.set_halign(gtk::Align::Start);
    ws_status_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
    ws_status_lbl.set_margin_bottom(8);
    inner.append(&ws_status_lbl);

    panel.append(&inner);
    panel
}

// ── Center empty state ─────────────────────────────────────────────────────────

fn build_center_empty_state(
    on_open_workspace: Rc<dyn Fn(std::path::PathBuf)>,
    on_new_workspace: Rc<dyn Fn()>,
) -> gtk::Box {
    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 12);
    box_.set_halign(gtk::Align::Center);
    box_.set_valign(gtk::Align::Center);
    box_.set_vexpand(true);
    box_.set_margin_top(48);

    let icon = gtk::Label::new(Some("🗂"));
    icon.add_css_class("placeholder-icon");
    box_.append(&icon);

    let title = gtk::Label::new(Some("No workspace open"));
    title.add_css_class("placeholder-title");
    box_.append(&title);

    let desc = gtk::Label::new(Some(
        "Open a .water workspace to browse Rooms, Shelves,\nPiles, and Loose Notes here.\n\nYou can still capture notes in your Inbox at any time.",
    ));
    desc.add_css_class("placeholder-desc");
    desc.set_justify(gtk::Justification::Center);
    desc.set_wrap(true);
    box_.append(&desc);

    let btns = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    btns.set_halign(gtk::Align::Center);
    btns.set_margin_top(8);

    let open_btn = gtk::Button::with_label("Open Workspace…");
    open_btn.add_css_class("desk-new-btn");
    open_btn.connect_clicked(move |btn| {
        show_open_workspace_dialog(btn, on_open_workspace.clone());
    });

    let new_btn = gtk::Button::with_label("New Workspace");
    new_btn.add_css_class("desk-new-btn");
    {
        let cb = on_new_workspace.clone();
        new_btn.connect_clicked(move |_| (cb)());
    }

    btns.append(&open_btn);
    btns.append(&new_btn);
    box_.append(&btns);
    box_
}

// ── Workspace snapshot collection ─────────────────────────────────────────────

fn collect_workspace_snapshot(db: &WorkspaceDb) -> WsSnap {
    let name = db.workspace_name();
    let path = db.path.to_string_lossy().into_owned();
    let rooms = db.list_rooms().unwrap_or_default();

    let room_snaps = rooms
        .iter()
        .map(|room| {
            let containers = db.list_containers_in_room(&room.id).unwrap_or_default();
            let shelves = containers
                .iter()
                .filter(|c| c.kind == ContainerKind::Shelf)
                .map(|c| ContainerSnap {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    is_pile: false,
                    note_count: db.container_note_count(&c.id),
                    notes: db
                        .list_notes_in_container(&c.id)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|n| NoteSnap {
                            id: n.id,
                            title: n.title,
                            snippet: first_line_preview(&n.body, 72),
                            updated_at: n.updated_at,
                        })
                        .collect(),
                })
                .collect();
            let piles = containers
                .iter()
                .filter(|c| c.kind == ContainerKind::Pile)
                .map(|c| ContainerSnap {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    is_pile: true,
                    note_count: db.container_note_count(&c.id),
                    notes: db
                        .list_notes_in_container(&c.id)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|n| NoteSnap {
                            id: n.id,
                            title: n.title,
                            snippet: first_line_preview(&n.body, 72),
                            updated_at: n.updated_at,
                        })
                        .collect(),
                })
                .collect();
            let loose_count = db.loose_note_count(&room.id);
            let loose_notes = db
                .list_loose_notes(&room.id)
                .unwrap_or_default()
                .into_iter()
                .map(|n| NoteSnap {
                    id: n.id,
                    title: n.title,
                    snippet: first_line_preview(&n.body, 72),
                    updated_at: n.updated_at,
                })
                .collect();
            RoomSnap {
                id: room.id.clone(),
                name: room.name.clone(),
                shelves,
                piles,
                loose_notes,
                loose_count,
            }
        })
        .collect();

    WsSnap {
        name,
        path,
        rooms: room_snaps,
    }
}

// ── Location label helpers ─────────────────────────────────────────────────────

/// Format a human-readable location label for a PinEntry.
pub fn pin_location_label(pin: &PinEntry) -> String {
    if pin.target_kind == "inbox_note" || pin.workspace_path.is_empty() {
        "Inbox".to_string()
    } else {
        let ws_name = std::path::Path::new(&pin.workspace_path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| pin.workspace_path.clone());
        format!("{ws_name} · Workspace")
    }
}

/// Format a human-readable location label for a RecentEntry.
pub fn recent_location_label(entry: &RecentEntry) -> String {
    if entry.target_kind == "inbox_note" || entry.workspace_path.is_empty() {
        "Inbox".to_string()
    } else if entry.workspace_name.is_empty() {
        "Workspace".to_string()
    } else {
        entry.workspace_name.clone()
    }
}

/// Format a location label for display: Inbox, Loose Notes, Room > Shelf, Room > Pile.
pub fn format_location_label(
    is_inbox: bool,
    room_name: Option<&str>,
    container_name: Option<&str>,
    is_pile: bool,
) -> String {
    if is_inbox {
        return "Inbox".to_string();
    }
    match (room_name, container_name) {
        (Some(r), Some(c)) => {
            let kind = if is_pile { "Pile" } else { "Shelf" };
            format!("{r} › {c} ({kind})")
        }
        (Some(r), None) => format!("{r} › Loose Notes"),
        (None, _) => "Workspace".to_string(),
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inbox::{PinEntry, RecentEntry};

    // ── format_location_label ────────────────────────────────────────────────

    #[test]
    fn location_label_inbox() {
        assert_eq!(format_location_label(true, None, None, false), "Inbox");
    }

    #[test]
    fn location_label_loose_in_room() {
        assert_eq!(
            format_location_label(false, Some("Research"), None, false),
            "Research › Loose Notes"
        );
    }

    #[test]
    fn location_label_shelf() {
        assert_eq!(
            format_location_label(false, Some("Research"), Some("Articles"), false),
            "Research › Articles (Shelf)"
        );
    }

    #[test]
    fn location_label_pile() {
        assert_eq!(
            format_location_label(false, Some("Research"), Some("Drafts"), true),
            "Research › Drafts (Pile)"
        );
    }

    #[test]
    fn location_label_no_room_returns_workspace() {
        assert_eq!(
            format_location_label(false, None, Some("Articles"), false),
            "Workspace"
        );
    }

    // ── pin_location_label ───────────────────────────────────────────────────

    #[test]
    fn pin_label_inbox_note() {
        let pin = PinEntry {
            id: "i1".into(),
            target_kind: "inbox_note".into(),
            target_id: "n1".into(),
            workspace_path: "".into(),
            note_title: "Hello".into(),
            note_snippet: "".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            sort_order: 0,
        };
        assert_eq!(pin_location_label(&pin), "Inbox");
    }

    #[test]
    fn pin_label_workspace_note_uses_file_stem() {
        let pin = PinEntry {
            id: "i2".into(),
            target_kind: "workspace_note".into(),
            target_id: "n2".into(),
            workspace_path: "/home/user/Research.water".into(),
            note_title: "Note".into(),
            note_snippet: "".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            sort_order: 0,
        };
        assert_eq!(pin_location_label(&pin), "Research · Workspace");
    }

    #[test]
    fn pin_label_nonexistent_path_does_not_panic() {
        let pin = PinEntry {
            id: "i3".into(),
            target_kind: "workspace_note".into(),
            target_id: "n3".into(),
            workspace_path: "/does/not/exist/Gone.water".into(),
            note_title: "Gone".into(),
            note_snippet: "".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            sort_order: 0,
        };
        let label = pin_location_label(&pin);
        assert!(label.contains("Gone"), "label was: {label}");
    }

    // ── recent_location_label ────────────────────────────────────────────────

    #[test]
    fn recent_label_inbox() {
        let e = RecentEntry {
            id: "r1".into(),
            target_kind: "inbox_note".into(),
            target_id: "n1".into(),
            workspace_path: "".into(),
            workspace_name: "".into(),
            note_title: "Draft".into(),
            note_snippet: "".into(),
            accessed_at: "2026-01-01T00:00:00Z".into(),
        };
        assert_eq!(recent_location_label(&e), "Inbox");
    }

    #[test]
    fn recent_label_uses_workspace_name() {
        let e = RecentEntry {
            id: "r2".into(),
            target_kind: "workspace_note".into(),
            target_id: "n2".into(),
            workspace_path: "/home/user/Project.water".into(),
            workspace_name: "Project".into(),
            note_title: "Note".into(),
            note_snippet: "".into(),
            accessed_at: "2026-01-01T00:00:00Z".into(),
        };
        assert_eq!(recent_location_label(&e), "Project");
    }

    #[test]
    fn recent_label_fallback_when_name_empty() {
        let e = RecentEntry {
            id: "r3".into(),
            target_kind: "workspace_note".into(),
            target_id: "n3".into(),
            workspace_path: "/some/path.water".into(),
            workspace_name: "".into(),
            note_title: "Note".into(),
            note_snippet: "".into(),
            accessed_at: "2026-01-01T00:00:00Z".into(),
        };
        assert_eq!(recent_location_label(&e), "Workspace");
    }
}

// ── Text snippets ─────────────────────────────────────────────────────────────

fn first_line_preview(body: &str, max_chars: usize) -> String {
    let line = body
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    let cleaned = line
        .trim_start_matches('#')
        .trim_start_matches('-')
        .trim_start_matches('>')
        .trim();
    if cleaned.chars().count() <= max_chars {
        cleaned.to_string()
    } else {
        let s: String = cleaned.chars().take(max_chars).collect();
        format!("{s}…")
    }
}

// ── Dialog helpers ─────────────────────────────────────────────────────────────

fn show_open_workspace_dialog(
    parent: &impl gtk::prelude::IsA<gtk::Widget>,
    on_chosen: Rc<dyn Fn(std::path::PathBuf)>,
) {
    let toplevel = parent
        .ancestor(gtk::Window::static_type())
        .and_then(|w| w.downcast::<gtk::Window>().ok());

    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Watercolor Workspace (*.water)"));
    filter.add_pattern("*.water");

    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);

    let dialog = gtk::FileDialog::builder()
        .title("Open Workspace")
        .accept_label("Open")
        .modal(true)
        .filters(&filters)
        .default_filter(&filter)
        .build();

    dialog.open(
        toplevel.as_ref(),
        None::<&gio::Cancellable>,
        move |result| {
            if let Ok(file) = result {
                if let Some(path) = file.path() {
                    on_chosen(path);
                }
            }
        },
    );
}

fn show_text_prompt(
    parent: Option<&gtk::Window>,
    title: &str,
    placeholder: Option<&str>,
    initial_text: Option<&str>,
    accept_label: &str,
    on_accept: impl Fn(String) + 'static,
) {
    let window = gtk::Window::builder()
        .title(title)
        .modal(true)
        .default_width(360)
        .resizable(false)
        .build();
    if let Some(p) = parent {
        window.set_transient_for(Some(p));
    }

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_start(16);
    content.set_margin_end(16);
    content.set_margin_top(16);
    content.set_margin_bottom(16);

    let entry = gtk::Entry::new();
    entry.set_activates_default(true);
    if let Some(p) = placeholder {
        entry.set_placeholder_text(Some(p));
    }
    if let Some(t) = initial_text {
        entry.set_text(t);
        entry.select_region(0, -1);
    }
    content.append(&entry);

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::End);
    let cancel_btn = gtk::Button::with_label("Cancel");
    let accept_btn = gtk::Button::with_label(accept_label);
    accept_btn.add_css_class("suggested-action");
    actions.append(&cancel_btn);
    actions.append(&accept_btn);
    content.append(&actions);

    window.set_child(Some(&content));

    let on_accept: Rc<dyn Fn(String)> = Rc::new(on_accept);
    let submit: Rc<dyn Fn()> = Rc::new({
        let entry = entry.clone();
        let window = window.clone();
        let on_accept = on_accept.clone();
        move || {
            let value = entry.text().trim().to_string();
            if !value.is_empty() {
                on_accept(value);
            }
            window.close();
        }
    });

    cancel_btn.connect_clicked({
        let w = window.clone();
        move |_| w.close()
    });
    accept_btn.connect_clicked({
        let s = submit.clone();
        move |_| s()
    });
    entry.connect_activate(move |_| submit());

    window.present();
    entry.grab_focus();
}
