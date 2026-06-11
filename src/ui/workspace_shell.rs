//! Workspace Mode UI for Blot.
//!
//! Shows the focused `.water` workspace: rooms, shelves, piles, loose notes,
//! and an embedded note editor. The Inbox remains separate — this shell never
//! reads or writes `inbox.db`.

use crate::document;
use crate::inbox::format_date_short;
use crate::title;
use crate::workspace::{
    new_id, now_iso8601, ContainerKind, NotePlacement, WorkspaceDb, WorkspaceNote,
    WorkspaceNoteSession,
};
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

const AUTOSAVE_DELAY_MS: u64 = 1_500;

// ─── WorkspaceShell ───────────────────────────────────────────────────────────

/// The Workspace Mode surface. Clone is cheap — all inner types are Rc-wrapped.
#[derive(Clone)]
pub struct WorkspaceShell {
    /// Root widget added to the mode stack.
    pub root: gtk::Box,

    // Sidebar widgets
    ws_name_label: gtk::Label,
    room_list: gtk::ListBox,
    #[allow(dead_code)]
    room_detail_scroll: gtk::ScrolledWindow,
    room_detail_box: gtk::Box,

    // Editor widgets
    breadcrumb: gtk::Label,
    title_entry: gtk::Entry,
    body_view: gtk::TextView,
    source_btn: gtk::ToggleButton,
    editor_save_label: gtk::Label,

    // Shared state
    workspace_db: Rc<RefCell<Option<WorkspaceDb>>>,
    ws_session: Rc<RefCell<WorkspaceNoteSession>>,
    current_room_id: Rc<RefCell<Option<String>>>,
    /// `None` = showing loose notes; `Some(id)` = showing a shelf/pile.
    current_container_id: Rc<RefCell<Option<String>>>,
    source_mode: Rc<Cell<bool>>,
    pending_timer: Rc<RefCell<Option<glib::SourceId>>>,
    title_auto_flag: Rc<Cell<bool>>,
    loading_flag: Rc<Cell<bool>>,
    /// Reference to the main status-bar save label.
    status_save_label: gtk::Label,
    /// Reference to the main status-bar location label.
    status_location_label: gtk::Label,
}

impl WorkspaceShell {
    pub fn new(
        workspace_db: Rc<RefCell<Option<WorkspaceDb>>>,
        ws_session: Rc<RefCell<WorkspaceNoteSession>>,
        status_save_label: gtk::Label,
        status_location_label: gtk::Label,
    ) -> Self {
        // ── Shared state ──────────────────────────────────────────────────
        let current_room_id: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let current_container_id: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let source_mode: Rc<Cell<bool>> = Rc::new(Cell::new(false));
        let pending_timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
        let title_auto_flag: Rc<Cell<bool>> = Rc::new(Cell::new(false));
        let loading_flag: Rc<Cell<bool>> = Rc::new(Cell::new(false));

        // ── Root: horizontal split ─────────────────────────────────────────
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("workspace-shell");

        // ══ Sidebar ═══════════════════════════════════════════════════════

        let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 0);
        sidebar.add_css_class("workspace-sidebar");
        sidebar.set_size_request(260, -1);

        // Workspace name header
        let ws_header = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        ws_header.add_css_class("workspace-sidebar-header");
        ws_header.set_margin_start(12);
        ws_header.set_margin_end(8);
        ws_header.set_margin_top(12);
        ws_header.set_margin_bottom(8);

        let ws_name_label = gtk::Label::new(Some("No workspace"));
        ws_name_label.add_css_class("workspace-name-label");
        ws_name_label.set_halign(gtk::Align::Start);
        ws_name_label.set_hexpand(true);
        ws_name_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        ws_header.append(&ws_name_label);

        sidebar.append(&ws_header);

        // "Rooms" section label + "New Room" button
        let rooms_header_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        rooms_header_box.set_margin_start(12);
        rooms_header_box.set_margin_end(8);
        rooms_header_box.set_margin_top(4);
        rooms_header_box.set_margin_bottom(2);

        let rooms_section_label = gtk::Label::new(Some("Rooms"));
        rooms_section_label.add_css_class("workspace-section-label");
        rooms_section_label.set_halign(gtk::Align::Start);
        rooms_section_label.set_hexpand(true);

        let new_room_btn = gtk::Button::with_label("+");
        new_room_btn.add_css_class("workspace-mini-btn");
        new_room_btn.set_tooltip_text(Some("Create a new Room"));

        rooms_header_box.append(&rooms_section_label);
        rooms_header_box.append(&new_room_btn);
        sidebar.append(&rooms_header_box);

        // Room list
        let room_scroll = gtk::ScrolledWindow::builder()
            .vexpand(false)
            .min_content_height(80)
            .max_content_height(180)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();

        let room_list = gtk::ListBox::new();
        room_list.add_css_class("workspace-room-list");
        room_list.set_selection_mode(gtk::SelectionMode::Single);
        room_scroll.set_child(Some(&room_list));
        sidebar.append(&room_scroll);

        // Room detail area (shelves, piles, loose notes)
        sidebar.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

        let room_detail_scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();

        let room_detail_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        room_detail_box.add_css_class("workspace-room-detail");
        room_detail_scroll.set_child(Some(&room_detail_box));
        sidebar.append(&room_detail_scroll);

        root.append(&sidebar);
        root.append(&gtk::Separator::new(gtk::Orientation::Vertical));

        // ══ Editor ════════════════════════════════════════════════════════

        let editor_panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
        editor_panel.set_hexpand(true);
        editor_panel.set_vexpand(true);

        let editor_scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .build();
        editor_scroll.add_css_class("editor-scroll");

        let editor_outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        editor_outer.set_halign(gtk::Align::Center);
        editor_outer.set_hexpand(true);
        editor_outer.set_margin_top(40);
        editor_outer.set_margin_bottom(64);
        editor_outer.set_margin_start(24);
        editor_outer.set_margin_end(24);
        editor_outer.set_size_request(640, -1);

        // Breadcrumb + source toggle row
        let toolbar_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        toolbar_row.set_margin_bottom(8);

        let breadcrumb = gtk::Label::new(Some("Workspace"));
        breadcrumb.add_css_class("editor-breadcrumb");
        breadcrumb.set_halign(gtk::Align::Start);
        breadcrumb.set_hexpand(true);

        let source_btn = gtk::ToggleButton::with_label("Source");
        source_btn.add_css_class("source-toggle-btn");
        source_btn.set_tooltip_text(Some("Toggle raw Markdown source view"));

        toolbar_row.append(&breadcrumb);
        toolbar_row.append(&source_btn);

        // Title entry
        let title_entry = gtk::Entry::new();
        title_entry.add_css_class("note-title-entry");
        title_entry.set_placeholder_text(Some("Untitled note"));
        title_entry.set_has_frame(false);
        title_entry.set_margin_bottom(8);

        // Body editor
        let body_view = gtk::TextView::new();
        body_view.add_css_class("note-body-view");
        body_view.set_wrap_mode(gtk::WrapMode::WordChar);
        body_view.set_top_margin(4);
        body_view.set_left_margin(2);
        body_view.set_right_margin(2);
        body_view.set_bottom_margin(4);
        body_view.set_vexpand(true);
        body_view.set_hexpand(true);
        body_view.set_cursor_visible(true);

        let editor_save_label = gtk::Label::new(Some(""));
        editor_save_label.add_css_class("editor-ws-save-label");
        editor_save_label.set_halign(gtk::Align::End);
        editor_save_label.set_margin_top(4);

        let hint_label = gtk::Label::new(Some("Start writing — your note saves automatically."));
        hint_label.add_css_class("editor-hint");
        hint_label.set_halign(gtk::Align::Start);
        hint_label.set_margin_top(8);

        editor_outer.append(&toolbar_row);
        editor_outer.append(&title_entry);
        editor_outer.append(&body_view);
        editor_outer.append(&hint_label);
        editor_outer.append(&editor_save_label);

        editor_scroll.set_child(Some(&editor_outer));
        editor_panel.append(&editor_scroll);
        root.append(&editor_panel);

        // ── Build struct ──────────────────────────────────────────────────

        let shell = WorkspaceShell {
            root,
            ws_name_label,
            room_list: room_list.clone(),
            room_detail_scroll,
            room_detail_box: room_detail_box.clone(),
            breadcrumb: breadcrumb.clone(),
            title_entry: title_entry.clone(),
            body_view: body_view.clone(),
            source_btn: source_btn.clone(),
            editor_save_label: editor_save_label.clone(),
            workspace_db: workspace_db.clone(),
            ws_session: ws_session.clone(),
            current_room_id: current_room_id.clone(),
            current_container_id: current_container_id.clone(),
            source_mode: source_mode.clone(),
            pending_timer: pending_timer.clone(),
            title_auto_flag: title_auto_flag.clone(),
            loading_flag: loading_flag.clone(),
            status_save_label: status_save_label.clone(),
            status_location_label: status_location_label.clone(),
        };

        // ── Wire room list row activation ─────────────────────────────────
        {
            let shell2 = shell.clone();
            room_list.connect_row_activated(move |_, row| {
                let room_id = row.widget_name().to_string();
                if !room_id.is_empty() {
                    shell2.select_room(&room_id);
                }
            });
        }

        // ── New room button ───────────────────────────────────────────────
        {
            let shell2 = shell.clone();
            new_room_btn.connect_clicked(move |btn| {
                shell2.prompt_create_room(btn);
            });
        }

        // ── Source toggle ─────────────────────────────────────────────────
        {
            let body_view = body_view.clone();
            let editor_save_label = editor_save_label.clone();
            let loading_flag = loading_flag.clone();
            let source_mode = source_mode.clone();

            source_btn.connect_toggled(move |btn| {
                let in_source = btn.is_active();
                source_mode.set(in_source);

                let buf = body_view.buffer();
                let current = buf
                    .text(&buf.start_iter(), &buf.end_iter(), false)
                    .to_string();
                if !current.trim().is_empty() {
                    let doc = document::markdown::parse(&current);
                    let normalized = document::serialize::to_source(&doc);
                    if normalized != current {
                        loading_flag.set(true);
                        buf.set_text(&normalized);
                        loading_flag.set(false);
                    }
                }

                if in_source {
                    btn.set_label("← Editor");
                    editor_save_label.set_text("Source view");
                } else {
                    btn.set_label("Source");
                    super::set_save_status(&editor_save_label, "Unsaved");
                }
            });
        }

        // ── Body change → debounced autosave ──────────────────────────────
        {
            let shell2 = shell.clone();
            let hint_label = hint_label.clone();

            body_view.buffer().connect_changed(move |buf| {
                if shell2.loading_flag.get() {
                    return;
                }
                hint_label.set_visible(buf.char_count() == 0);
                shell2.ws_session.borrow_mut().dirty = true;
                super::set_save_status(&shell2.editor_save_label, "Unsaved");
                super::set_save_status(&shell2.status_save_label, "Unsaved");

                if let Some(id) = shell2.pending_timer.borrow_mut().take() {
                    id.remove();
                }

                let shell3 = shell2.clone();
                let id =
                    glib::timeout_add_local(Duration::from_millis(AUTOSAVE_DELAY_MS), move || {
                        shell3.perform_save();
                        glib::ControlFlow::Break
                    });
                *shell2.pending_timer.borrow_mut() = Some(id);
            });
        }

        // ── Title change → mark as user-titled ───────────────────────────
        {
            let ws_session = ws_session.clone();
            let title_auto_flag = title_auto_flag.clone();
            let loading_flag = loading_flag.clone();

            title_entry.connect_changed(move |_| {
                if title_auto_flag.get() || loading_flag.get() {
                    return;
                }
                ws_session.borrow_mut().auto_titled = false;
            });
        }

        shell
    }

    // ── Public refresh API ────────────────────────────────────────────────

    /// Refresh the entire shell from the current workspace state.
    /// Call this when a workspace is opened or switched.
    pub fn refresh(&self) {
        let db_guard = self.workspace_db.borrow();
        let Some(db) = db_guard.as_ref() else {
            self.ws_name_label.set_text("No workspace");
            self.clear_room_list();
            self.clear_room_detail();
            return;
        };

        self.ws_name_label.set_text(&db.workspace_name());
        self.status_location_label.set_text(&db.workspace_name());

        // Reload rooms.
        self.clear_room_list();
        let rooms = db.list_rooms().unwrap_or_default();
        for room in &rooms {
            self.append_room_row(&room.id, &room.name);
        }

        // Select default room if none selected.
        let current = self.current_room_id.borrow().clone();
        let room_to_select = current
            .or_else(|| db.default_room_id())
            .or_else(|| rooms.first().map(|r| r.id.clone()));

        if let Some(rid) = room_to_select {
            *self.current_room_id.borrow_mut() = Some(rid.clone());
            self.select_room_in_list(&rid);
            drop(db_guard);
            self.refresh_room_detail(&rid);
        } else {
            self.clear_room_detail();
        }
    }

    /// Force save the current workspace note synchronously.
    pub fn force_save_sync(&self) {
        if let Some(id) = self.pending_timer.borrow_mut().take() {
            id.remove();
        }
        self.perform_save();
    }

    /// Clear the editor for a brand-new blank workspace note.
    pub fn new_note_in_current_room(&self) {
        self.force_save_sync();

        let room_id = self.current_room_id.borrow().clone();
        let Some(room_id) = room_id else {
            self.editor_save_label.set_text("Select a room first");
            return;
        };

        let container_id = self.current_container_id.borrow().clone();

        // Set up a fresh session.
        {
            let mut s = self.ws_session.borrow_mut();
            s.note_id = None;
            s.room_id = Some(room_id.clone());
            s.shelf_id = container_id;
            s.auto_titled = true;
            s.dirty = false;
        }

        self.loading_flag.set(true);
        self.title_auto_flag.set(true);
        self.title_entry.set_text("");
        self.body_view.buffer().set_text("");
        self.loading_flag.set(false);
        self.title_auto_flag.set(false);

        if self.source_mode.get() {
            self.source_mode.set(false);
            self.source_btn.set_active(false);
        }

        if let Some(id) = self.pending_timer.borrow_mut().take() {
            id.remove();
        }

        self.editor_save_label.set_text("New note");
        self.status_save_label.set_text("New note");
        self.update_breadcrumb();
        self.body_view.grab_focus();
    }

    /// Load a workspace note into the editor.
    pub fn open_note(&self, note_id: &str) {
        self.force_save_sync();

        let note_opt = {
            let db_guard = self.workspace_db.borrow();
            db_guard
                .as_ref()
                .and_then(|db| db.get_note(note_id).ok().flatten())
        };
        let placement_opt = {
            let db_guard = self.workspace_db.borrow();
            db_guard
                .as_ref()
                .and_then(|db| db.get_note_placement(note_id).ok().flatten())
        };

        let Some(note) = note_opt else {
            self.editor_save_label.set_text("Note not found");
            return;
        };

        let display_text = note
            .document_json
            .as_deref()
            .and_then(|j| serde_json::from_str::<document::NoteDocument>(j).ok())
            .map(|doc| document::serialize::to_source(&doc))
            .unwrap_or_else(|| note.body.clone());

        self.loading_flag.set(true);
        self.title_auto_flag.set(true);
        self.title_entry.set_text(&note.title);
        self.body_view.buffer().set_text(&display_text);
        self.loading_flag.set(false);
        self.title_auto_flag.set(false);

        if self.source_mode.get() {
            self.source_mode.set(false);
            self.source_btn.set_active(false);
        }

        if let Some(id) = self.pending_timer.borrow_mut().take() {
            id.remove();
        }

        {
            let mut s = self.ws_session.borrow_mut();
            s.note_id = Some(note.id.clone());
            s.room_id = placement_opt.as_ref().map(|p| p.room_id.clone());
            s.shelf_id = placement_opt.and_then(|p| p.shelf_id.clone());
            s.auto_titled = note.auto_titled;
            s.dirty = false;
        }

        super::set_save_status(&self.editor_save_label, "Opened");
        super::set_save_status(&self.status_save_label, "Opened");
        self.update_breadcrumb();
    }

    /// Navigate to a specific room from Room Map Mode.
    /// Refreshes the workspace view and selects the room.
    pub fn navigate_to_room(&self, room_id: &str) {
        // If the room list is empty (workspace just opened), do a full refresh first.
        let has_rooms = self.room_list.first_child().is_some();
        if !has_rooms {
            self.refresh();
        }
        self.select_room(room_id);
    }

    // ── Private helpers ───────────────────────────────────────────────────

    fn clear_room_list(&self) {
        while let Some(child) = self.room_list.first_child() {
            self.room_list.remove(&child);
        }
    }

    fn clear_room_detail(&self) {
        while let Some(child) = self.room_detail_box.first_child() {
            self.room_detail_box.remove(&child);
        }
    }

    fn append_room_row(&self, room_id: &str, room_name: &str) {
        let row = gtk::ListBoxRow::new();
        row.add_css_class("workspace-room-row");
        row.set_widget_name(room_id);

        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        row_box.set_margin_start(12);
        row_box.set_margin_end(8);
        row_box.set_margin_top(6);
        row_box.set_margin_bottom(6);

        let name_lbl = gtk::Label::new(Some(room_name));
        name_lbl.add_css_class("workspace-room-name");
        name_lbl.set_halign(gtk::Align::Start);
        name_lbl.set_hexpand(true);
        name_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);

        // Rename button
        let rename_btn = gtk::Button::with_label("✎");
        rename_btn.add_css_class("workspace-mini-btn");
        rename_btn.set_tooltip_text(Some("Rename this Room"));

        {
            let shell = self.clone();
            let rid = room_id.to_string();
            rename_btn.connect_clicked(move |btn| {
                shell.prompt_rename_room(&rid, btn);
            });
        }

        row_box.append(&name_lbl);
        row_box.append(&rename_btn);
        row.set_child(Some(&row_box));
        self.room_list.append(&row);
    }

    fn select_room_in_list(&self, room_id: &str) {
        let mut row = self.room_list.first_child();
        while let Some(child) = row {
            let next = child.next_sibling();
            if let Ok(list_row) = child.clone().downcast::<gtk::ListBoxRow>() {
                if list_row.widget_name().as_str() == room_id {
                    self.room_list.select_row(Some(&list_row));
                    break;
                }
            }
            row = next;
        }
    }

    fn select_room(&self, room_id: &str) {
        self.force_save_sync();
        *self.current_room_id.borrow_mut() = Some(room_id.to_string());
        *self.current_container_id.borrow_mut() = None;
        self.refresh_room_detail(room_id);
        self.update_breadcrumb();
    }

    /// Rebuild the room detail area for the given room.
    fn refresh_room_detail(&self, room_id: &str) {
        self.clear_room_detail();

        // Collect all DB data while holding the borrow, then release it before
        // building the UI so that UI callbacks can re-borrow safely.
        struct RoomData {
            shelves: Vec<(String, String, i64)>, // (id, name, note_count)
            piles: Vec<(String, String, i64)>,
            loose_count: i64,
            loose_notes: Vec<crate::workspace::WorkspaceNote>,
        }

        let data_opt: Option<RoomData> = {
            let db_guard = self.workspace_db.borrow();
            db_guard.as_ref().map(|db| {
                let containers = db.list_containers_in_room(room_id).unwrap_or_default();
                let shelves = containers
                    .iter()
                    .filter(|c| c.kind == ContainerKind::Shelf)
                    .map(|c| (c.id.clone(), c.name.clone(), db.container_note_count(&c.id)))
                    .collect();
                let piles = containers
                    .iter()
                    .filter(|c| c.kind == ContainerKind::Pile)
                    .map(|c| (c.id.clone(), c.name.clone(), db.container_note_count(&c.id)))
                    .collect();
                let loose_count = db.loose_note_count(room_id);
                let loose_notes = db.list_loose_notes(room_id).unwrap_or_default();
                RoomData {
                    shelves,
                    piles,
                    loose_count,
                    loose_notes,
                }
            })
        }; // db_guard released here

        let Some(data) = data_opt else {
            return;
        };

        // ── Shelves section ───────────────────────────────────────────────
        self.append_section_header("Shelves", Some("+ Shelf"), {
            let shell = self.clone();
            let rid = room_id.to_string();
            move |_| shell.prompt_create_container(&rid, ContainerKind::Shelf)
        });

        if data.shelves.is_empty() {
            self.append_empty_hint("No shelves yet.");
        } else {
            for (id, name, note_count) in &data.shelves {
                self.append_container_row(id, name, "Shelf", *note_count, false, room_id);
            }
        }

        // ── Piles section ─────────────────────────────────────────────────
        self.append_section_header("Piles", Some("+ Pile"), {
            let shell = self.clone();
            let rid = room_id.to_string();
            move |_| shell.prompt_create_container(&rid, ContainerKind::Pile)
        });

        if data.piles.is_empty() {
            self.append_empty_hint("No piles yet.");
        } else {
            for (id, name, note_count) in &data.piles {
                self.append_container_row(id, name, "Pile", *note_count, true, room_id);
            }
        }

        // ── Loose Notes section ───────────────────────────────────────────
        self.append_section_header(
            &format!("Loose Notes ({})", data.loose_count),
            Some("+ Note"),
            {
                let shell = self.clone();
                move |_| shell.new_note_in_current_room()
            },
        );

        if data.loose_notes.is_empty() {
            self.append_empty_hint("No loose notes.");
        } else {
            self.append_note_rows(&data.loose_notes, None, room_id);
        }
    }

    fn append_section_header(
        &self,
        label_text: &str,
        action_label: Option<&str>,
        on_action: impl Fn(&gtk::Button) + 'static,
    ) {
        let hdr = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        hdr.add_css_class("workspace-section-header");
        hdr.set_margin_start(12);
        hdr.set_margin_end(8);
        hdr.set_margin_top(10);
        hdr.set_margin_bottom(2);

        let lbl = gtk::Label::new(Some(label_text));
        lbl.add_css_class("workspace-section-label");
        lbl.set_halign(gtk::Align::Start);
        lbl.set_hexpand(true);

        hdr.append(&lbl);

        if let Some(action_text) = action_label {
            let btn = gtk::Button::with_label(action_text);
            btn.add_css_class("workspace-mini-btn");
            btn.connect_clicked(on_action);
            hdr.append(&btn);
        }

        self.room_detail_box.append(&hdr);
    }

    fn append_empty_hint(&self, text: &str) {
        let lbl = gtk::Label::new(Some(text));
        lbl.add_css_class("workspace-empty-hint");
        lbl.set_halign(gtk::Align::Start);
        lbl.set_margin_start(16);
        lbl.set_margin_top(2);
        lbl.set_margin_bottom(2);
        self.room_detail_box.append(&lbl);
    }

    fn append_container_row(
        &self,
        container_id: &str,
        name: &str,
        _kind_label: &str,
        note_count: i64,
        is_pile: bool,
        room_id: &str,
    ) {
        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        row_box.add_css_class("workspace-container-row");
        row_box.set_margin_start(16);
        row_box.set_margin_end(8);
        row_box.set_margin_top(2);
        row_box.set_margin_bottom(2);

        let icon = if is_pile { "📦" } else { "📚" };
        let label_text = format!("{icon} {name} ({note_count})");
        let lbl = gtk::Label::new(Some(&label_text));
        lbl.add_css_class("workspace-container-name");
        lbl.set_halign(gtk::Align::Start);
        lbl.set_hexpand(true);
        lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);

        // Click label area → show notes in this container
        let btn = gtk::Button::new();
        btn.set_child(Some(&lbl));
        btn.add_css_class("workspace-container-btn");
        btn.set_has_frame(false);
        btn.set_hexpand(true);
        {
            let shell = self.clone();
            let cid = container_id.to_string();
            let rid = room_id.to_string();
            btn.connect_clicked(move |_| {
                shell.show_container_notes(&cid, &rid);
            });
        }

        row_box.append(&btn);

        // If pile: "→ Shelf" convert button
        if is_pile {
            let convert_btn = gtk::Button::with_label("→ Shelf");
            convert_btn.add_css_class("workspace-mini-btn");
            convert_btn.set_tooltip_text(Some(&format!("Convert '{name}' from Pile to Shelf")));
            {
                let shell = self.clone();
                let cid = container_id.to_string();
                let rid = room_id.to_string();
                convert_btn.connect_clicked(move |_| {
                    shell.convert_pile_to_shelf(&cid, &rid);
                });
            }
            row_box.append(&convert_btn);
        }

        self.room_detail_box.append(&row_box);

        // Show notes for this container below the row if it's currently selected.
        let is_current = self.current_container_id.borrow().as_deref() == Some(container_id);
        if is_current {
            let notes = {
                let db_guard = self.workspace_db.borrow();
                db_guard
                    .as_ref()
                    .and_then(|db| db.list_notes_in_container(container_id).ok())
                    .unwrap_or_default()
            };
            self.append_note_rows(&notes, Some(container_id), room_id);
        }
    }

    fn append_note_rows(
        &self,
        notes: &[WorkspaceNote],
        container_id: Option<&str>,
        _room_id: &str,
    ) {
        for note in notes {
            let note_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
            note_row.add_css_class("workspace-note-row");
            note_row.set_margin_start(24);
            note_row.set_margin_end(8);
            note_row.set_margin_top(1);
            note_row.set_margin_bottom(1);

            let display_title = if note.title.is_empty() {
                "Untitled note".to_string()
            } else {
                note.title.clone()
            };

            let title_lbl = gtk::Label::new(Some(&display_title));
            title_lbl.add_css_class("workspace-note-title");
            title_lbl.set_halign(gtk::Align::Start);
            title_lbl.set_hexpand(true);
            title_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);

            let date_lbl = gtk::Label::new(Some(format_date_short(&note.updated_at)));
            date_lbl.add_css_class("workspace-note-date");
            date_lbl.set_halign(gtk::Align::End);

            let note_btn = gtk::Button::new();
            note_btn.add_css_class("workspace-note-btn");
            note_btn.set_has_frame(false);
            note_btn.set_hexpand(true);

            let inner = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            inner.append(&title_lbl);
            inner.append(&date_lbl);
            note_btn.set_child(Some(&inner));

            {
                let shell = self.clone();
                let nid = note.id.clone();
                note_btn.connect_clicked(move |_| {
                    shell.open_note(&nid);
                });
            }

            note_row.append(&note_btn);

            // Move to loose / move to container actions
            if container_id.is_some() {
                let move_btn = gtk::Button::with_label("↑");
                move_btn.add_css_class("workspace-mini-btn");
                move_btn.set_tooltip_text(Some("Move to Loose Notes"));
                {
                    let shell = self.clone();
                    let nid = note.id.clone();
                    let rid = _room_id.to_string();
                    move_btn.connect_clicked(move |_| {
                        let result = shell
                            .workspace_db
                            .borrow()
                            .as_ref()
                            .map(|db| db.move_note_to_loose(&nid, &rid));
                        if let Some(Ok(())) = result {
                            if let Some(rid2) = shell.current_room_id.borrow().clone() {
                                shell.refresh_room_detail(&rid2);
                            }
                        }
                    });
                }
                note_row.append(&move_btn);
            }

            self.room_detail_box.append(&note_row);
        }
    }

    fn show_container_notes(&self, container_id: &str, room_id: &str) {
        *self.current_container_id.borrow_mut() = Some(container_id.to_string());
        self.refresh_room_detail(room_id);
    }

    fn convert_pile_to_shelf(&self, pile_id: &str, room_id: &str) {
        let result = self
            .workspace_db
            .borrow()
            .as_ref()
            .map(|db| db.convert_pile_to_shelf(pile_id));
        match result {
            Some(Ok(())) => self.refresh_room_detail(room_id),
            Some(Err(e)) => {
                eprintln!("blot: convert pile to shelf failed: {e}");
            }
            None => {}
        }
    }

    fn update_breadcrumb(&self) {
        let db_guard = self.workspace_db.borrow();
        let ws_name = db_guard
            .as_ref()
            .map(|db| db.workspace_name())
            .unwrap_or_else(|| "Workspace".to_string());

        let room_name = self.current_room_id.borrow().as_deref().and_then(|rid| {
            db_guard
                .as_ref()
                .and_then(|db| db.get_room(rid).ok().flatten())
                .map(|r| r.name)
        });

        let breadcrumb_text = match room_name {
            Some(rn) => format!("{ws_name} › {rn}"),
            None => ws_name,
        };

        self.breadcrumb.set_text(&breadcrumb_text);
        self.status_location_label.set_text(&breadcrumb_text);
    }

    // ── Dialog helpers ─────────────────────────────────────────────────────

    fn prompt_create_room(&self, parent_widget: &gtk::Button) {
        let toplevel = parent_widget
            .ancestor(gtk::Window::static_type())
            .and_then(|w| w.downcast::<gtk::Window>().ok());

        let shell = self.clone();
        show_text_prompt(
            toplevel.as_ref(),
            "New Room",
            Some("Room name"),
            None,
            "Create",
            move |name| {
                let result = shell
                    .workspace_db
                    .borrow()
                    .as_ref()
                    .map(|db| db.create_room(&name));
                if let Some(Ok(_)) = result {
                    shell.refresh();
                }
            },
        );
    }

    fn prompt_rename_room(&self, room_id: &str, parent_widget: &gtk::Button) {
        let current_name = {
            let db_guard = self.workspace_db.borrow();
            db_guard
                .as_ref()
                .and_then(|db| db.get_room(room_id).ok().flatten())
                .map(|r| r.name)
                .unwrap_or_default()
        };

        let toplevel = parent_widget
            .ancestor(gtk::Window::static_type())
            .and_then(|w| w.downcast::<gtk::Window>().ok());

        let shell = self.clone();
        let rid = room_id.to_string();
        show_text_prompt(
            toplevel.as_ref(),
            "Rename Room",
            None,
            Some(&current_name),
            "Rename",
            move |new_name| {
                let result = shell
                    .workspace_db
                    .borrow()
                    .as_ref()
                    .map(|db| db.rename_room(&rid, &new_name));
                if let Some(Ok(())) = result {
                    shell.refresh();
                }
            },
        );
    }

    fn prompt_create_container(&self, room_id: &str, kind: ContainerKind) {
        // Use an inline text popup since we don't have a parent button here.
        // Show a simple input dialog attached to the shell root.
        let toplevel = self
            .root
            .ancestor(gtk::Window::static_type())
            .and_then(|w| w.downcast::<gtk::Window>().ok());

        let kind_label = kind.display_label();
        let shell = self.clone();
        let rid = room_id.to_string();
        show_text_prompt(
            toplevel.as_ref(),
            &format!("New {kind_label}"),
            Some(&format!("{kind_label} name")),
            None,
            "Create",
            move |name| {
                let result = shell
                    .workspace_db
                    .borrow()
                    .as_ref()
                    .map(|db| db.create_container(&rid, &name, kind.clone()));
                if let Some(Ok(_)) = result {
                    shell.refresh_room_detail(&rid);
                }
            },
        );
    }

    // ── Core save ─────────────────────────────────────────────────────────

    fn perform_save(&self) {
        let buf = self.body_view.buffer();
        let body = buf
            .text(&buf.start_iter(), &buf.end_iter(), false)
            .to_string();

        if title::is_blank(&body) {
            let has_saved = self.ws_session.borrow().note_id.is_some();
            if !has_saved {
                self.pending_timer.borrow_mut().take();
                return;
            }
            super::set_save_status(&self.editor_save_label, "Blank");
            self.pending_timer.borrow_mut().take();
            return;
        }

        let doc = document::markdown::parse(&body);
        let document_json = serde_json::to_string(&doc).ok();
        let wc = title::word_count(&body) as i64;
        let is_auto = self.ws_session.borrow().auto_titled;

        let use_title = if is_auto {
            doc.first_heading_text()
                .map(str::to_string)
                .or_else(|| {
                    let t = title::derive_title(&body);
                    if t.is_empty() {
                        None
                    } else {
                        Some(t)
                    }
                })
                .unwrap_or_else(|| "Untitled note".to_string())
        } else {
            let t = self.title_entry.text().to_string();
            if t.is_empty() {
                "Untitled note".to_string()
            } else {
                t
            }
        };

        if is_auto && self.title_entry.text().as_str() != use_title.as_str() {
            self.title_auto_flag.set(true);
            self.title_entry.set_text(&use_title);
            self.title_auto_flag.set(false);
        }

        let (note_id, room_id, shelf_id) = {
            let mut s = self.ws_session.borrow_mut();
            let nid = s.note_id.get_or_insert_with(new_id).clone();
            let rid = s.room_id.clone().unwrap_or_default();
            let sid = s.shelf_id.clone();
            (nid, rid, sid)
        };

        if room_id.is_empty() {
            self.editor_save_label.set_text("No room");
            return;
        }

        let now = now_iso8601();
        let note = WorkspaceNote {
            id: note_id.clone(),
            title: use_title,
            body,
            document_json,
            auto_titled: is_auto,
            created_at: now.clone(),
            updated_at: now,
            word_count: wc,
            is_archived: false,
        };
        let placement = NotePlacement {
            note_id: note_id.clone(),
            room_id,
            shelf_id,
            position: 0.0,
        };

        let result = self.workspace_db.borrow().as_ref().map(|db| {
            db.upsert_note(&note)
                .and_then(|_| db.set_note_placement(&placement))
        });

        match result {
            Some(Ok(())) => {
                {
                    let mut s = self.ws_session.borrow_mut();
                    s.note_id = Some(note_id);
                    s.auto_titled = is_auto;
                    s.dirty = false;
                }
                self.pending_timer.borrow_mut().take();
                super::set_save_status(&self.editor_save_label, "Saved");
                super::set_save_status(&self.status_save_label, "Saved");
            }
            Some(Err(e)) => {
                eprintln!("blot: workspace autosave error: {e}");
                super::set_save_status(&self.editor_save_label, "Save error");
                super::set_save_status(&self.status_save_label, "Save error");
            }
            None => {
                self.editor_save_label.set_text("No workspace");
            }
        }
    }
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
    if let Some(parent) = parent {
        window.set_transient_for(Some(parent));
    }

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_start(16);
    content.set_margin_end(16);
    content.set_margin_top(16);
    content.set_margin_bottom(16);

    let entry = gtk::Entry::new();
    entry.set_activates_default(true);
    if let Some(placeholder) = placeholder {
        entry.set_placeholder_text(Some(placeholder));
    }
    if let Some(initial_text) = initial_text {
        entry.set_text(initial_text);
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
        let window = window.clone();
        move |_| window.close()
    });
    accept_btn.connect_clicked({
        let submit = submit.clone();
        move |_| submit()
    });
    entry.connect_activate(move |_| submit());

    window.present();
    entry.grab_focus();
}
