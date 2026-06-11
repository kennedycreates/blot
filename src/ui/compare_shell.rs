//! Two-panel Compare Mode for Blot.
//!
//! Displays two note editors side by side.  Each panel autosaves independently
//! — to the Inbox for Inbox notes, to the open workspace for workspace notes.
//! Copy / Move selection buttons transfer text between panels.

use super::modal_host::{self, ButtonKind, ModalHost};
use crate::document;
use crate::inbox::{new_note_id, now_iso8601, InboxDb, InboxNote};
use crate::title;
use crate::workspace::{WorkspaceDb, WorkspaceNote};
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

const AUTOSAVE_DELAY_MS: u64 = 1_500;

// ── NoteSource (compare-local copy, no GTK dep) ───────────────────────────────

/// Which store a compare panel's note lives in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompareSource {
    Inbox,
    Workspace(PathBuf),
}

// ── CompareSession ────────────────────────────────────────────────────────────

/// In-memory state for one panel of Compare Mode.  No GTK dependency.
#[derive(Debug, Clone, Default)]
pub struct CompareSession {
    pub note_id: Option<String>,
    pub source: Option<CompareSource>,
    /// Cached title (may differ from title_entry during typing).
    pub title: String,
    pub auto_titled: bool,
    pub dirty: bool,
}

impl CompareSession {
    pub fn reset(&mut self) {
        *self = CompareSession::default();
    }

    pub fn is_inbox(&self) -> bool {
        matches!(self.source, Some(CompareSource::Inbox))
    }
}

// ── ComparePanel (one side) ───────────────────────────────────────────────────

/// One half of the Compare Mode layout: header + editable text area + autosave.
#[derive(Clone)]
struct ComparePanel {
    root: gtk::Box,
    title_label: gtk::Label,
    source_label: gtk::Label,
    body_view: gtk::TextView,
    session: Rc<RefCell<CompareSession>>,
    inbox_db: Rc<RefCell<Option<InboxDb>>>,
    workspace_db: Rc<RefCell<Option<WorkspaceDb>>>,
    pending_timer: Rc<RefCell<Option<glib::SourceId>>>,
    loading_flag: Rc<Cell<bool>>,
    title_auto_flag: Rc<Cell<bool>>,
}

impl ComparePanel {
    fn new(
        inbox_db: Rc<RefCell<Option<InboxDb>>>,
        workspace_db: Rc<RefCell<Option<WorkspaceDb>>>,
        panel_label: &str,
    ) -> Self {
        let session: Rc<RefCell<CompareSession>> = Rc::new(RefCell::new(CompareSession::default()));
        let pending_timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
        let loading_flag: Rc<Cell<bool>> = Rc::new(Cell::new(false));
        let title_auto_flag: Rc<Cell<bool>> = Rc::new(Cell::new(false));

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("compare-panel");

        // Panel header
        let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        header.add_css_class("compare-panel-header");
        header.set_margin_start(12);
        header.set_margin_end(12);
        header.set_margin_top(8);
        header.set_margin_bottom(6);

        let panel_badge = gtk::Label::new(Some(panel_label));
        panel_badge.add_css_class("compare-panel-badge");

        let title_label = gtk::Label::new(Some("No note selected"));
        title_label.add_css_class("compare-panel-title");
        title_label.set_halign(gtk::Align::Start);
        title_label.set_hexpand(true);
        title_label.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let source_label = gtk::Label::new(Some(""));
        source_label.add_css_class("compare-panel-source");
        source_label.set_halign(gtk::Align::End);

        header.append(&panel_badge);
        header.append(&title_label);
        header.append(&source_label);

        let sep = gtk::Separator::new(gtk::Orientation::Horizontal);

        // Body editor
        let scrolled = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .build();
        scrolled.add_css_class("compare-panel-scroll");

        let body_view = gtk::TextView::new();
        body_view.add_css_class("compare-panel-body");
        body_view.set_wrap_mode(gtk::WrapMode::WordChar);
        body_view.set_top_margin(12);
        body_view.set_left_margin(16);
        body_view.set_right_margin(16);
        body_view.set_bottom_margin(12);
        body_view.set_vexpand(true);
        body_view.set_hexpand(true);
        body_view.set_cursor_visible(true);

        let empty_label = gtk::Label::new(Some(
            "No note loaded.\nUse the picker above to choose a note.",
        ));
        empty_label.add_css_class("compare-empty-hint");
        empty_label.set_justify(gtk::Justification::Center);
        empty_label.set_margin_top(48);

        scrolled.set_child(Some(&body_view));

        root.append(&header);
        root.append(&sep);
        root.append(&scrolled);
        root.append(&empty_label);

        // Autosave on buffer change
        {
            let session2 = session.clone();
            let inbox_db2 = inbox_db.clone();
            let workspace_db2 = workspace_db.clone();
            let body_view2 = body_view.clone();
            let title_label2 = title_label.clone();
            let pending_timer2 = pending_timer.clone();
            let loading_flag2 = loading_flag.clone();
            let title_auto_flag2 = title_auto_flag.clone();
            let empty_label2 = empty_label.clone();

            body_view.buffer().connect_changed(move |buf| {
                if loading_flag2.get() {
                    return;
                }
                empty_label2.set_visible(false);
                session2.borrow_mut().dirty = true;

                if let Some(id) = pending_timer2.borrow_mut().take() {
                    id.remove();
                }

                let s3 = session2.clone();
                let idb3 = inbox_db2.clone();
                let wdb3 = workspace_db2.clone();
                let bv3 = body_view2.clone();
                let tl3 = title_label2.clone();
                let pt3 = pending_timer2.clone();
                let taf3 = title_auto_flag2.clone();

                let timer_id =
                    glib::timeout_add_local(Duration::from_millis(AUTOSAVE_DELAY_MS), move || {
                        perform_compare_save(&bv3, &tl3, &idb3, &wdb3, &s3, &pt3, &taf3);
                        glib::ControlFlow::Break
                    });
                *pending_timer2.borrow_mut() = Some(timer_id);
            });
        }

        ComparePanel {
            root,
            title_label,
            source_label,
            body_view,
            session,
            inbox_db,
            workspace_db,
            pending_timer,
            loading_flag,
            title_auto_flag,
        }
    }

    /// Load an Inbox note into this panel.
    fn load_inbox_note(&self, note: &InboxNote) {
        let display_text = note
            .document_json
            .as_deref()
            .and_then(|j| serde_json::from_str::<document::NoteDocument>(j).ok())
            .map(|doc| document::serialize::to_source(&doc))
            .unwrap_or_else(|| note.body.clone());

        self.loading_flag.set(true);
        self.body_view.buffer().set_text(&display_text);
        self.loading_flag.set(false);

        cancel_timer(&self.pending_timer);

        let mut s = self.session.borrow_mut();
        s.note_id = Some(note.id.clone());
        s.source = Some(CompareSource::Inbox);
        s.title = note.title.clone();
        s.auto_titled = note.auto_titled;
        s.dirty = false;

        self.title_label.set_text(&note.title);
        self.source_label.set_text("Inbox");
    }

    /// Load a workspace note into this panel.
    fn load_workspace_note(&self, note: &WorkspaceNote, workspace_name: &str, path: PathBuf) {
        let display_text = note
            .document_json
            .as_deref()
            .and_then(|j| serde_json::from_str::<document::NoteDocument>(j).ok())
            .map(|doc| document::serialize::to_source(&doc))
            .unwrap_or_else(|| note.body.clone());

        self.loading_flag.set(true);
        self.body_view.buffer().set_text(&display_text);
        self.loading_flag.set(false);

        cancel_timer(&self.pending_timer);

        let mut s = self.session.borrow_mut();
        s.note_id = Some(note.id.clone());
        s.source = Some(CompareSource::Workspace(path));
        s.title = note.title.clone();
        s.auto_titled = note.auto_titled;
        s.dirty = false;

        self.title_label.set_text(&note.title);
        self.source_label.set_text(workspace_name);
    }

    fn force_save_sync(&self) {
        cancel_timer(&self.pending_timer);
        perform_compare_save(
            &self.body_view,
            &self.title_label,
            &self.inbox_db,
            &self.workspace_db,
            &self.session,
            &self.pending_timer,
            &self.title_auto_flag,
        );
    }

    fn get_selected_text(&self) -> String {
        let buf = self.body_view.buffer();
        if let Some((start, end)) = buf.selection_bounds() {
            buf.text(&start, &end, false).to_string()
        } else {
            String::new()
        }
    }

    fn append_text(&self, text: &str, source_title: &str) {
        let buf = self.body_view.buffer();
        let mut end = buf.end_iter();
        let divider = format!("\n\n--- Moved from: {source_title} ---\n");
        buf.insert(&mut end, &divider);
        let mut end2 = buf.end_iter();
        buf.insert(&mut end2, text);
    }

    fn delete_selected(&self) {
        let buf = self.body_view.buffer();
        if buf.has_selection() {
            buf.delete_selection(true, true);
        }
    }

    /// Auto-bookmark the currently loaded note before a destructive operation.
    /// Silently skips if no note is loaded or the DB is unavailable.
    fn auto_bookmark_if_loaded(&self, reason: &str) {
        let session = self.session.borrow();
        let Some(note_id) = session.note_id.as_deref() else {
            return;
        };
        match &session.source {
            Some(CompareSource::Inbox) => {
                if let Some(db) = self.inbox_db.borrow().as_ref() {
                    if let Ok(Some(note)) = db.get_note(note_id) {
                        let _ = db.create_version(&note, reason, false, None, Some("auto"), None);
                    }
                }
            }
            Some(CompareSource::Workspace(_)) => {
                if let Some(db) = self.workspace_db.borrow().as_ref() {
                    if let Ok(Some(note)) = db.get_note(note_id) {
                        let _ =
                            db.create_note_version(&note, reason, false, None, Some("auto"), None);
                    }
                }
            }
            None => {}
        }
    }
}

fn cancel_timer(pending: &Rc<RefCell<Option<glib::SourceId>>>) {
    if let Some(id) = pending.borrow_mut().take() {
        id.remove();
    }
}

/// Autosave logic for a compare panel.  Routes to the right DB.
fn perform_compare_save(
    body_view: &gtk::TextView,
    title_label: &gtk::Label,
    inbox_db: &Rc<RefCell<Option<InboxDb>>>,
    workspace_db: &Rc<RefCell<Option<WorkspaceDb>>>,
    session: &Rc<RefCell<CompareSession>>,
    pending_timer: &Rc<RefCell<Option<glib::SourceId>>>,
    title_auto_flag: &Rc<Cell<bool>>,
) {
    let buf = body_view.buffer();
    let body = buf
        .text(&buf.start_iter(), &buf.end_iter(), false)
        .to_string();

    if title::is_blank(&body) {
        pending_timer.borrow_mut().take();
        return;
    }

    let doc = document::markdown::parse(&body);
    let document_json = serde_json::to_string(&doc).ok();
    let wc = title::word_count(&body) as i64;

    let is_auto = session.borrow().auto_titled;
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
        let t = session.borrow().title.clone();
        if t.is_empty() {
            "Untitled note".to_string()
        } else {
            t
        }
    };

    if is_auto {
        title_auto_flag.set(true);
        title_label.set_text(&use_title);
        title_auto_flag.set(false);
    }

    let source = session.borrow().source.clone();
    let note_id_opt = session.borrow().note_id.clone();

    match source {
        Some(CompareSource::Inbox) => {
            let note_id = note_id_opt.unwrap_or_else(new_note_id);
            let now = now_iso8601();
            let note = InboxNote {
                id: note_id.clone(),
                title: use_title.clone(),
                body,
                document_json,
                auto_titled: is_auto,
                created_at: now.clone(),
                updated_at: now,
                word_count: wc,
                is_pinned: false,
                is_archived: false,
                placed_at: None,
                placed_workspace_path: None,
                placed_workspace_note_id: None,
                placed_destination_label: None,
            };
            if let Some(Ok(())) = inbox_db.borrow().as_ref().map(|db| db.upsert_note(&note)) {
                let mut s = session.borrow_mut();
                s.note_id = Some(note_id);
                s.title = use_title;
                s.dirty = false;
            }
        }
        Some(CompareSource::Workspace(_)) => {
            let Some(note_id) = note_id_opt else {
                return;
            };
            let now = now_iso8601();
            let note = WorkspaceNote {
                id: note_id.clone(),
                title: use_title.clone(),
                body,
                document_json,
                auto_titled: is_auto,
                created_at: now.clone(),
                updated_at: now,
                word_count: wc,
                is_archived: false,
            };
            if let Some(Ok(())) = workspace_db
                .borrow()
                .as_ref()
                .map(|db| db.upsert_note(&note))
            {
                let mut s = session.borrow_mut();
                s.title = use_title;
                s.dirty = false;
            }
        }
        None => {}
    }

    pending_timer.borrow_mut().take();
}

// ── CompareShell ──────────────────────────────────────────────────────────────

/// The complete two-panel Compare Mode surface.
/// Clone is cheap — all inner state is Rc-wrapped.
#[derive(Clone)]
pub struct CompareShell {
    pub root: gtk::Box,
    left: ComparePanel,
    right: ComparePanel,
    inbox_db: Rc<RefCell<Option<InboxDb>>>,
    workspace_db: Rc<RefCell<Option<WorkspaceDb>>>,
    modal_host: ModalHost,
    on_exit: Rc<dyn Fn()>,
}

impl CompareShell {
    pub fn new(
        inbox_db: Rc<RefCell<Option<InboxDb>>>,
        workspace_db: Rc<RefCell<Option<WorkspaceDb>>>,
        modal_host: ModalHost,
        on_exit: impl Fn() + 'static,
    ) -> Self {
        let left = ComparePanel::new(inbox_db.clone(), workspace_db.clone(), "A");
        let right = ComparePanel::new(inbox_db.clone(), workspace_db.clone(), "B");

        // ── Toolbar ───────────────────────────────────────────────────────
        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        toolbar.add_css_class("compare-toolbar");
        toolbar.set_margin_start(12);
        toolbar.set_margin_end(12);
        toolbar.set_margin_top(8);
        toolbar.set_margin_bottom(8);

        let exit_btn = gtk::Button::with_label("← Exit Compare");
        exit_btn.add_css_class("compare-exit-btn");
        exit_btn.set_tooltip_text(Some("Return to normal editing (saves both panels first)"));

        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);

        let copy_left_btn = gtk::Button::with_label("← Copy");
        copy_left_btn.add_css_class("compare-action-btn");
        copy_left_btn.set_tooltip_text(Some(
            "Copy selected text from B and append it to A (does not remove from B)",
        ));

        let move_left_btn = gtk::Button::with_label("← Move");
        move_left_btn.add_css_class("compare-action-btn");
        move_left_btn.set_tooltip_text(Some("Move selected text from B to A (removes from B)"));

        let swap_btn = gtk::Button::with_label("⇄ Swap");
        swap_btn.add_css_class("compare-action-btn");
        swap_btn.set_tooltip_text(Some("Swap left and right panels"));

        let move_right_btn = gtk::Button::with_label("Move →");
        move_right_btn.add_css_class("compare-action-btn");
        move_right_btn.set_tooltip_text(Some("Move selected text from A to B (removes from A)"));

        let copy_right_btn = gtk::Button::with_label("Copy →");
        copy_right_btn.add_css_class("compare-action-btn");
        copy_right_btn.set_tooltip_text(Some(
            "Copy selected text from A and append it to B (does not remove from A)",
        ));

        toolbar.append(&exit_btn);
        toolbar.append(&spacer);
        toolbar.append(&copy_left_btn);
        toolbar.append(&move_left_btn);
        toolbar.append(&swap_btn);
        toolbar.append(&move_right_btn);
        toolbar.append(&copy_right_btn);

        let sep = gtk::Separator::new(gtk::Orientation::Horizontal);

        // ── Split pane ────────────────────────────────────────────────────
        let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        paned.add_css_class("compare-paned");
        paned.set_vexpand(true);
        paned.set_hexpand(true);
        paned.set_wide_handle(true);

        paned.set_start_child(Some(&left.root));
        paned.set_end_child(Some(&right.root));
        paned.set_resize_start_child(true);
        paned.set_resize_end_child(true);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("compare-shell");
        root.append(&toolbar);
        root.append(&sep);
        root.append(&paned);

        let shell = CompareShell {
            root,
            left,
            right,
            inbox_db,
            workspace_db,
            modal_host,
            on_exit: Rc::new(on_exit),
        };

        // ── Button wiring ─────────────────────────────────────────────────

        {
            let s = shell.clone();
            exit_btn.connect_clicked(move |_| {
                s.left.force_save_sync();
                s.right.force_save_sync();
                (s.on_exit)();
            });
        }

        {
            let left2 = shell.left.clone();
            let right2 = shell.right.clone();
            copy_right_btn.connect_clicked(move |_| {
                let text = left2.get_selected_text();
                if !text.is_empty() {
                    let src_title = left2.session.borrow().title.clone();
                    right2.append_text(&text, &src_title);
                }
            });
        }

        {
            let left2 = shell.left.clone();
            let right2 = shell.right.clone();
            move_right_btn.connect_clicked(move |_| {
                let text = left2.get_selected_text();
                if !text.is_empty() {
                    left2.auto_bookmark_if_loaded("before move right");
                    right2.auto_bookmark_if_loaded("before move right");
                    let src_title = left2.session.borrow().title.clone();
                    right2.append_text(&text, &src_title);
                    left2.delete_selected();
                }
            });
        }

        {
            let left2 = shell.left.clone();
            let right2 = shell.right.clone();
            copy_left_btn.connect_clicked(move |_| {
                let text = right2.get_selected_text();
                if !text.is_empty() {
                    let src_title = right2.session.borrow().title.clone();
                    left2.append_text(&text, &src_title);
                }
            });
        }

        {
            let left2 = shell.left.clone();
            let right2 = shell.right.clone();
            move_left_btn.connect_clicked(move |_| {
                let text = right2.get_selected_text();
                if !text.is_empty() {
                    right2.auto_bookmark_if_loaded("before move left");
                    left2.auto_bookmark_if_loaded("before move left");
                    let src_title = right2.session.borrow().title.clone();
                    left2.append_text(&text, &src_title);
                    right2.delete_selected();
                }
            });
        }

        {
            let s = shell.clone();
            swap_btn.connect_clicked(move |_| {
                // Save both panels first, then swap their content.
                s.left.force_save_sync();
                s.right.force_save_sync();

                let left_session = s.left.session.borrow().clone();
                let right_session = s.right.session.borrow().clone();

                let left_buf = s.left.body_view.buffer();
                let right_buf = s.right.body_view.buffer();

                let left_text = left_buf
                    .text(&left_buf.start_iter(), &left_buf.end_iter(), false)
                    .to_string();
                let right_text = right_buf
                    .text(&right_buf.start_iter(), &right_buf.end_iter(), false)
                    .to_string();

                s.left.loading_flag.set(true);
                s.right.loading_flag.set(true);
                left_buf.set_text(&right_text);
                right_buf.set_text(&left_text);
                s.left.loading_flag.set(false);
                s.right.loading_flag.set(false);

                // Swap sessions.
                *s.left.session.borrow_mut() = right_session;
                *s.right.session.borrow_mut() = left_session;

                // Swap header labels.
                let lt = s.left.title_label.text().to_string();
                let rt = s.right.title_label.text().to_string();
                let ls = s.left.source_label.text().to_string();
                let rs = s.right.source_label.text().to_string();
                s.left.title_label.set_text(&rt);
                s.right.title_label.set_text(&lt);
                s.left.source_label.set_text(&rs);
                s.right.source_label.set_text(&ls);
            });
        }

        shell
    }

    // ── Public API ────────────────────────────────────────────────────────

    /// Load an Inbox note into the left panel.
    pub fn load_left_inbox(&self, note: &InboxNote) {
        self.left.load_inbox_note(note);
    }

    /// Load an Inbox note into the right panel.
    pub fn load_right_inbox(&self, note: &InboxNote) {
        self.right.load_inbox_note(note);
    }

    /// Load a workspace note into the left panel.
    pub fn load_left_workspace(&self, note: &WorkspaceNote, ws_name: &str, path: PathBuf) {
        self.left.load_workspace_note(note, ws_name, path);
    }

    /// Load a workspace note into the right panel.
    pub fn load_right_workspace(&self, note: &WorkspaceNote, ws_name: &str, path: PathBuf) {
        self.right.load_workspace_note(note, ws_name, path);
    }

    /// Save both panels immediately (called before exiting or switching notes).
    pub fn force_save_both(&self) {
        self.left.force_save_sync();
        self.right.force_save_sync();
    }

    /// Open a note-picker dialog for the left panel and load the chosen note.
    pub fn pick_left_note(&self) {
        self.build_note_picker_dialog();
    }

    /// Open a note-picker dialog for the right panel and load the chosen note.
    pub fn pick_right_note(&self) {
        self.build_note_picker_dialog();
    }

    /// Build a modal note-picker overlay that loads the chosen note.
    fn build_note_picker_dialog(&self) {
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
        vbox.add_css_class("compare-picker-window");
        vbox.set_size_request(420, 360);

        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some("Filter notes…"));
        search.set_margin_bottom(6);

        let scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();

        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Browse);
        list.add_css_class("compare-picker-list");

        // Populate with Inbox notes + workspace notes.
        {
            if let Some(db) = self.inbox_db.borrow().as_ref() {
                if let Ok(notes) = db.list_notes() {
                    for note in notes {
                        let row = gtk::ListBoxRow::new();
                        let label = gtk::Label::builder()
                            .label(format!("[Inbox] {}", note.title))
                            .halign(gtk::Align::Start)
                            .margin_start(12)
                            .margin_end(12)
                            .margin_top(6)
                            .margin_bottom(6)
                            .build();
                        row.set_child(Some(&label));
                        // Store note_id as widget name for retrieval on activation.
                        row.set_widget_name(&note.id);
                        list.append(&row);
                    }
                }
            }
            if let Some(db) = self.workspace_db.borrow().as_ref() {
                let ws_name = db.workspace_name();
                let ws_path = db.path.clone();
                if let Ok(notes) = db.list_all_notes() {
                    for note in notes {
                        let row = gtk::ListBoxRow::new();
                        let label = gtk::Label::builder()
                            .label(format!("[{ws_name}] {}", note.title))
                            .halign(gtk::Align::Start)
                            .margin_start(12)
                            .margin_end(12)
                            .margin_top(6)
                            .margin_bottom(6)
                            .build();
                        row.set_child(Some(&label));
                        // "W:<note_id>" prefix distinguishes workspace rows.
                        row.set_widget_name(&format!("W:{}", note.id));
                        let _ = ws_path.clone();
                        list.append(&row);
                    }
                }
            }
        }

        scroll.set_child(Some(&list));
        vbox.append(&search);
        vbox.append(&scroll);

        // Filter on search.
        {
            let list2 = list.clone();
            search.connect_search_changed(move |entry| {
                let text = entry.text().to_lowercase();
                let mut idx: i32 = 0;
                loop {
                    let Some(row) = list2.row_at_index(idx) else {
                        break;
                    };
                    let visible = text.is_empty()
                        || row
                            .child()
                            .and_then(|w| w.downcast::<gtk::Label>().ok())
                            .map(|l| l.label().to_lowercase().contains(&text))
                            .unwrap_or(false);
                    row.set_visible(visible);
                    idx += 1;
                }
            });
        }

        // Activate row → load note into the correct panel.
        {
            let shell = self.clone();
            list.connect_row_activated(move |_, row| {
                let key = row.widget_name().to_string();
                shell.load_note_by_key(&key);
                shell.modal_host.hide();
            });
        }

        // Also handle Enter key in search.
        {
            let list2 = list.clone();
            let shell = self.clone();
            let key_ctrl = gtk::EventControllerKey::new();
            key_ctrl.connect_key_pressed(move |_, key, _, _| match key {
                gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter => {
                    if let Some(row) = list2.selected_row() {
                        let key = row.widget_name().to_string();
                        shell.load_note_by_key(&key);
                        shell.modal_host.hide();
                    }
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            });
            search.add_controller(key_ctrl);
        }

        let actions = modal_host::build_modal_actions();
        let host_close = self.modal_host.clone();
        let cancel_btn = modal_host::build_modal_button("Cancel", ButtonKind::Secondary, move || {
            host_close.hide()
        });
        actions.append(&cancel_btn);

        self.modal_host
            .show_with_custom_ui("Choose a note", &vbox, &actions, true, None);
        search.grab_focus();
    }

    /// Load the note identified by `key` into the LAST-focused panel.
    /// Key format: `"W:<note_id>"` for workspace, otherwise bare note_id for inbox.
    fn load_note_by_key(&self, key: &str) {
        if let Some(ws_note_id) = key.strip_prefix("W:") {
            // Workspace note — load into the right panel (or left if right is occupied).
            if let Some(db) = self.workspace_db.borrow().as_ref() {
                if let Ok(Some(note)) = db.get_note(ws_note_id) {
                    let ws_name = db.workspace_name();
                    let path = db.path.clone();
                    self.right.load_workspace_note(&note, &ws_name, path);
                }
            }
        } else {
            // Inbox note — load into the right panel.
            if let Some(db) = self.inbox_db.borrow().as_ref() {
                if let Ok(Some(note)) = db.get_note(key) {
                    self.right.load_inbox_note(&note);
                }
            }
        }
    }

    /// Open the note picker for the right panel.  Call from the header "Pick B" button.
    pub fn open_right_picker(&self) {
        self.build_note_picker_dialog();
    }

    /// True if either panel has been loaded with a note.
    pub fn has_any_note(&self) -> bool {
        self.left.session.borrow().note_id.is_some()
            || self.right.session.borrow().note_id.is_some()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_session_default_is_clean() {
        let s = CompareSession::default();
        assert!(s.note_id.is_none());
        assert!(s.source.is_none());
        assert!(!s.dirty);
        assert!(!s.is_inbox());
    }

    #[test]
    fn compare_session_reset_clears_all_fields() {
        let mut s = CompareSession {
            note_id: Some("abc".to_string()),
            source: Some(CompareSource::Inbox),
            title: "Hello".to_string(),
            auto_titled: true,
            dirty: true,
        };
        s.reset();
        assert!(s.note_id.is_none());
        assert!(s.source.is_none());
        assert!(s.title.is_empty());
        assert!(!s.dirty);
    }

    #[test]
    fn compare_session_is_inbox_true_for_inbox() {
        let mut s = CompareSession::default();
        s.source = Some(CompareSource::Inbox);
        assert!(s.is_inbox());
    }

    #[test]
    fn compare_session_is_inbox_false_for_workspace() {
        let mut s = CompareSession::default();
        s.source = Some(CompareSource::Workspace(PathBuf::from("/x.water")));
        assert!(!s.is_inbox());
    }

    #[test]
    fn compare_source_workspace_path_roundtrip() {
        let path = PathBuf::from("/home/user/test.water");
        let src = CompareSource::Workspace(path.clone());
        if let CompareSource::Workspace(p) = src {
            assert_eq!(p, path);
        } else {
            panic!("expected Workspace");
        }
    }

    #[test]
    fn compare_session_can_hold_workspace_source() {
        let mut s = CompareSession::default();
        s.source = Some(CompareSource::Workspace(PathBuf::from("/ws.water")));
        s.note_id = Some("n1".to_string());
        s.title = "My Note".to_string();
        assert_eq!(s.note_id.as_deref(), Some("n1"));
        assert!(!s.is_inbox());
    }
}
