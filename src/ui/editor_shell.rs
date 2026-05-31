use crate::document;
use crate::inbox::{new_note_id, now_iso8601, InboxDb, InboxNote, NoteSession};
use crate::title;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

// Autosave fires this long after the last keystroke.
const AUTOSAVE_DELAY_MS: u64 = 1_500;

// ── EditorWidgets ─────────────────────────────────────────────────────────────

/// Deferred single-arg callback type used for split/bookmark/history/merge buttons.
pub type DeferredCb<A> = Rc<RefCell<Option<Box<dyn Fn(A)>>>>;

/// Handles to the live GTK widgets that make up the editor surface.
/// Clone is cheap — all inner types are reference-counted.
#[derive(Clone)]
pub struct EditorWidgets {
    /// Root widget added to the mode stack.
    pub root: gtk::ScrolledWindow,
    pub title_entry: gtk::Entry,
    pub body_view: gtk::TextView,
    /// Save / status indicator shown in the status bar.
    pub save_label: gtk::Label,
    /// Toggle button that switches between normal editing and source view.
    pub source_btn: gtk::ToggleButton,
    /// Button that opens the Place Note dialog. Insensitive until the note is saved.
    pub place_btn: gtk::Button,
    /// Button that splits selected text into a new note.
    pub split_btn: gtk::Button,
    /// Button that creates a named bookmark of the current state.
    pub bookmark_btn: gtk::Button,
    /// Button that opens the version history dialog.
    pub history_btn: gtk::Button,
    /// Button that opens the merge dialog.
    pub merge_btn: gtk::Button,
    /// True when source-view mode is active.
    pub source_mode: Rc<Cell<bool>>,
    /// Pending autosave timer handle.
    pub pending_timer: Rc<RefCell<Option<glib::SourceId>>>,
    /// Set while we update the title entry programmatically (suppresses
    /// the user-title signal).
    pub title_auto_flag: Rc<Cell<bool>>,
    /// Set while loading note content from DB (suppresses autosave).
    pub loading_flag: Rc<Cell<bool>>,
    /// Called after each successful save with (note_id, title).
    /// Used by the tab bar to keep tab titles and IDs in sync.
    pub on_note_saved: Rc<dyn Fn(String, String)>,
    /// Deferred: called with note_id when Split Note is activated.
    pub on_split: DeferredCb<String>,
    /// Deferred: called with note_id when Bookmark is activated.
    pub on_bookmark: DeferredCb<String>,
    /// Deferred: called with note_id when Show History is activated.
    pub on_history: DeferredCb<String>,
    /// Deferred: called with note_id when Merge is activated.
    pub on_merge: DeferredCb<String>,
}

impl EditorWidgets {
    // ── Public API called by main_window ──────────────────────────────────

    /// Load an existing Inbox note without triggering autosave.
    /// Prefers `document_json` for display if present; falls back to body.
    pub fn load_note(&self, note: &InboxNote, session: &Rc<RefCell<NoteSession>>) {
        // Derive the canonical display text from the structured document if
        // available, falling back to the stored body.
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

        // Reset source-mode toggle to normal on note load.
        if self.source_mode.get() {
            self.source_mode.set(false);
            self.source_btn.set_active(false);
        }

        if let Some(id) = self.pending_timer.borrow_mut().take() {
            id.remove();
        }

        let mut s = session.borrow_mut();
        s.note_id = Some(note.id.clone());
        s.auto_titled = note.auto_titled;
        s.dirty = false;

        self.save_label.set_text("Opened");
        self.place_btn.set_sensitive(true);
        self.split_btn.set_sensitive(true);
        self.bookmark_btn.set_sensitive(true);
        self.history_btn.set_sensitive(true);
        self.merge_btn.set_sensitive(true);
    }

    /// Clear the editor for a brand-new blank note.
    pub fn new_note(&self, session: &Rc<RefCell<NoteSession>>) {
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

        session.borrow_mut().reset();
        self.save_label.set_text("New note");
        self.place_btn.set_sensitive(false);
        self.split_btn.set_sensitive(false);
        self.bookmark_btn.set_sensitive(false);
        self.history_btn.set_sensitive(false);
        self.merge_btn.set_sensitive(false);
        self.body_view.grab_focus();
    }

    fn enable_action_buttons(&self, enabled: bool) {
        self.split_btn.set_sensitive(enabled);
        self.bookmark_btn.set_sensitive(enabled);
        self.history_btn.set_sensitive(enabled);
        self.merge_btn.set_sensitive(enabled);
    }

    /// Save immediately, bypassing the debounce timer.
    /// Call before closing the window or switching to another note.
    pub fn force_save_sync(
        &self,
        db: &Rc<RefCell<Option<InboxDb>>>,
        session: &Rc<RefCell<NoteSession>>,
    ) {
        if let Some(id) = self.pending_timer.borrow_mut().take() {
            id.remove();
        }
        perform_save(
            &self.body_view,
            &self.title_entry,
            &self.save_label,
            db,
            session,
            &self.pending_timer,
            &self.title_auto_flag,
        );
        // Notify tab bar of the saved note identity.
        if let Some(nid) = session.borrow().note_id.clone() {
            let title = self.title_entry.text().to_string();
            (self.on_note_saved)(nid, title);
        }
    }
}

// ── Build ─────────────────────────────────────────────────────────────────────

/// Construct the Editor Mode surface and wire up autosave + source toggle.
///
/// `on_note_saved(note_id, title)` is called after every successful autosave so
/// the tab bar can keep its title and ID in sync.
pub fn build(
    db: Rc<RefCell<Option<InboxDb>>>,
    session: Rc<RefCell<NoteSession>>,
    save_label: gtk::Label,
    on_place_note: impl Fn(String, String) + 'static,
    on_note_saved: impl Fn(String, String) + 'static,
) -> EditorWidgets {
    let on_note_saved: Rc<dyn Fn(String, String)> = Rc::new(on_note_saved);
    // ── Shared state ──────────────────────────────────────────────────────
    let pending_timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let title_auto_flag: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let loading_flag: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let source_mode: Rc<Cell<bool>> = Rc::new(Cell::new(false));

    // ── Layout ────────────────────────────────────────────────────────────
    let scrolled = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hexpand(true)
        .build();
    scrolled.add_css_class("editor-scroll");

    let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    outer.set_halign(gtk::Align::Center);
    outer.set_hexpand(true);
    outer.set_margin_top(48);
    outer.set_margin_bottom(64);
    outer.set_margin_start(24);
    outer.set_margin_end(24);
    outer.set_size_request(640, -1);

    // ── Top toolbar: breadcrumb + source toggle ───────────────────────────
    let toolbar_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    toolbar_row.set_margin_bottom(8);

    let breadcrumb = gtk::Label::new(Some("Inbox"));
    breadcrumb.add_css_class("editor-breadcrumb");
    breadcrumb.set_halign(gtk::Align::Start);
    breadcrumb.set_hexpand(true);

    let place_btn = gtk::Button::with_label("Place Note…");
    place_btn.add_css_class("place-note-btn");
    place_btn.set_tooltip_text(Some("Move this note into a workspace (Ctrl+Shift+P)"));
    place_btn.set_sensitive(false);

    let split_btn = gtk::Button::with_label("Split");
    split_btn.add_css_class("editor-action-btn");
    split_btn.set_tooltip_text(Some("Split selected text into a new note"));
    split_btn.set_sensitive(false);

    let bookmark_btn = gtk::Button::with_label("Bookmark");
    bookmark_btn.add_css_class("editor-action-btn");
    bookmark_btn.set_tooltip_text(Some("Save a named bookmark of the current state"));
    bookmark_btn.set_sensitive(false);

    let history_btn = gtk::Button::with_label("History");
    history_btn.add_css_class("editor-action-btn");
    history_btn.set_tooltip_text(Some("View version history for this note"));
    history_btn.set_sensitive(false);

    let merge_btn = gtk::Button::with_label("Merge…");
    merge_btn.add_css_class("editor-action-btn");
    merge_btn.set_tooltip_text(Some("Merge other notes into this one"));
    merge_btn.set_sensitive(false);

    let source_btn = gtk::ToggleButton::with_label("Source");
    source_btn.add_css_class("source-toggle-btn");
    source_btn.set_tooltip_text(Some("Toggle raw Markdown source view"));

    toolbar_row.append(&breadcrumb);
    toolbar_row.append(&split_btn);
    toolbar_row.append(&bookmark_btn);
    toolbar_row.append(&history_btn);
    toolbar_row.append(&merge_btn);
    toolbar_row.append(&place_btn);
    toolbar_row.append(&source_btn);

    // ── Note title ────────────────────────────────────────────────────────
    let title_entry = gtk::Entry::new();
    title_entry.add_css_class("note-title-entry");
    title_entry.set_placeholder_text(Some("Untitled note"));
    title_entry.set_has_frame(false);
    title_entry.set_margin_bottom(8);

    // ── Note body — plain text for Prompt 3; block editor in Prompt 4+ ───
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

    // Hint shown while the buffer is empty
    let hint_label = gtk::Label::new(Some("Start writing — your note saves automatically."));
    hint_label.add_css_class("editor-hint");
    hint_label.set_halign(gtk::Align::Start);
    hint_label.set_margin_top(8);

    outer.append(&toolbar_row);
    outer.append(&title_entry);
    outer.append(&body_view);
    outer.append(&hint_label);
    scrolled.set_child(Some(&outer));

    let on_split: DeferredCb<String> = Rc::new(RefCell::new(None));
    let on_bookmark: DeferredCb<String> = Rc::new(RefCell::new(None));
    let on_history: DeferredCb<String> = Rc::new(RefCell::new(None));
    let on_merge: DeferredCb<String> = Rc::new(RefCell::new(None));

    let widgets = EditorWidgets {
        root: scrolled,
        title_entry: title_entry.clone(),
        body_view: body_view.clone(),
        save_label: save_label.clone(),
        source_btn: source_btn.clone(),
        place_btn: place_btn.clone(),
        split_btn: split_btn.clone(),
        bookmark_btn: bookmark_btn.clone(),
        history_btn: history_btn.clone(),
        merge_btn: merge_btn.clone(),
        source_mode: source_mode.clone(),
        pending_timer: pending_timer.clone(),
        title_auto_flag: title_auto_flag.clone(),
        loading_flag: loading_flag.clone(),
        on_note_saved: on_note_saved.clone(),
        on_split: on_split.clone(),
        on_bookmark: on_bookmark.clone(),
        on_history: on_history.clone(),
        on_merge: on_merge.clone(),
    };

    // ── Place Note button ─────────────────────────────────────────────────────
    {
        let session = session.clone();
        let title_entry = title_entry.clone();
        place_btn.connect_clicked(move |_| {
            let note_id = session.borrow().note_id.clone();
            if let Some(note_id) = note_id {
                let title = title_entry.text().to_string();
                on_place_note(note_id, title);
            }
        });
    }

    // ── Note action buttons (Split / Bookmark / History / Merge) ─────────
    {
        let session = session.clone();
        let cb = on_split.clone();
        split_btn.connect_clicked(move |_| {
            if let Some(nid) = session.borrow().note_id.clone() {
                if let Some(f) = cb.borrow().as_ref() {
                    f(nid);
                }
            }
        });
    }
    {
        let session = session.clone();
        let cb = on_bookmark.clone();
        bookmark_btn.connect_clicked(move |_| {
            if let Some(nid) = session.borrow().note_id.clone() {
                if let Some(f) = cb.borrow().as_ref() {
                    f(nid);
                }
            }
        });
    }
    {
        let session = session.clone();
        let cb = on_history.clone();
        history_btn.connect_clicked(move |_| {
            if let Some(nid) = session.borrow().note_id.clone() {
                if let Some(f) = cb.borrow().as_ref() {
                    f(nid);
                }
            }
        });
    }
    {
        let session = session.clone();
        let cb = on_merge.clone();
        merge_btn.connect_clicked(move |_| {
            if let Some(nid) = session.borrow().note_id.clone() {
                if let Some(f) = cb.borrow().as_ref() {
                    f(nid);
                }
            }
        });
    }

    // ── Source toggle ─────────────────────────────────────────────────────
    {
        let body_view = body_view.clone();
        let save_label = save_label.clone();
        let loading_flag = loading_flag.clone();
        let source_mode = source_mode.clone();

        source_btn.connect_toggled(move |btn| {
            let in_source = btn.is_active();
            source_mode.set(in_source);

            // Pass the current content through parse→serialize to normalize it.
            // This makes both modes show a canonically formatted view.
            let buf = body_view.buffer();
            let current = buf
                .text(&buf.start_iter(), &buf.end_iter(), false)
                .to_string();
            if !current.trim().is_empty() {
                let doc = document::markdown::parse(&current);
                let normalized = document::serialize::to_source(&doc);
                // Suppress the loading flag so we DON'T block the subsequent
                // buffer.connect_changed — we want autosave to pick up any
                // changes the normalisation introduced.
                if normalized != current {
                    loading_flag.set(true);
                    buf.set_text(&normalized);
                    loading_flag.set(false);
                }
            }

            if in_source {
                btn.set_label("← Editor");
                save_label.set_text("Source view");
            } else {
                btn.set_label("Source");
                // Mark content dirty so autosave syncs the normalised version.
                save_label.set_text("Unsaved");
            }
        });
    }

    // ── Body change → debounced autosave ──────────────────────────────────
    {
        let db = db.clone();
        let session = session.clone();
        let title_entry = title_entry.clone();
        let body_view = body_view.clone();
        let save_label = save_label.clone();
        let pending_timer = pending_timer.clone();
        let title_auto_flag = title_auto_flag.clone();
        let loading_flag = loading_flag.clone();
        let hint_label = hint_label.clone();

        body_view.buffer().connect_changed(move |buf| {
            if loading_flag.get() {
                return;
            }
            hint_label.set_visible(buf.char_count() == 0);
            session.borrow_mut().dirty = true;
            save_label.set_text("Unsaved");

            if let Some(id) = pending_timer.borrow_mut().take() {
                id.remove();
            }

            let db2 = db.clone();
            let session2 = session.clone();
            let title2 = title_entry.clone();
            let body2 = body_view.clone();
            let label2 = save_label.clone();
            let timer2 = pending_timer.clone();
            let flag2 = title_auto_flag.clone();
            let on_saved2 = on_note_saved.clone();

            let id = glib::timeout_add_local(Duration::from_millis(AUTOSAVE_DELAY_MS), move || {
                perform_save(&body2, &title2, &label2, &db2, &session2, &timer2, &flag2);
                // Notify tab bar after successful save.
                if let Some(nid) = session2.borrow().note_id.clone() {
                    on_saved2(nid, title2.text().to_string());
                }
                glib::ControlFlow::Break
            });
            *pending_timer.borrow_mut() = Some(id);
        });
    }

    // ── Title change → mark as user-titled ───────────────────────────────
    {
        let session = session.clone();
        let title_auto_flag = title_auto_flag.clone();
        let loading_flag = loading_flag.clone();

        title_entry.connect_changed(move |_| {
            if title_auto_flag.get() || loading_flag.get() {
                return;
            }
            session.borrow_mut().auto_titled = false;
        });
    }

    widgets
}

// ── Core save logic ───────────────────────────────────────────────────────────

/// Read current editor content, parse into a `NoteDocument`, derive or
/// preserve the title, and upsert both the body and document JSON to the DB.
fn perform_save(
    body_view: &gtk::TextView,
    title_entry: &gtk::Entry,
    save_label: &gtk::Label,
    db: &Rc<RefCell<Option<InboxDb>>>,
    session: &Rc<RefCell<NoteSession>>,
    pending_timer: &Rc<RefCell<Option<glib::SourceId>>>,
    title_auto_flag: &Rc<Cell<bool>>,
) {
    let buf = body_view.buffer();
    let body = buf
        .text(&buf.start_iter(), &buf.end_iter(), false)
        .to_string();

    if title::is_blank(&body) {
        let has_saved = session.borrow().note_id.is_some();
        if !has_saved {
            pending_timer.borrow_mut().take();
            return;
        }
        save_label.set_text("Blank");
        pending_timer.borrow_mut().take();
        return;
    }

    // ── Parse body into structured document ───────────────────────────────
    let doc = document::markdown::parse(&body);

    // Serialise the document to JSON for storage.
    let document_json = serde_json::to_string(&doc).ok();

    // ── Determine title ───────────────────────────────────────────────────
    let wc = title::word_count(&body) as i64;
    let is_auto = session.borrow().auto_titled;

    let use_title = if is_auto {
        // Prefer first heading block; fall back to title::derive_title.
        let candidate = doc
            .first_heading_text()
            .map(str::to_string)
            .or_else(|| {
                let t = title::derive_title(&body);
                if t.is_empty() {
                    None
                } else {
                    Some(t)
                }
            })
            .unwrap_or_else(|| "Untitled note".to_string());
        candidate
    } else {
        let t = title_entry.text().to_string();
        if t.is_empty() {
            "Untitled note".to_string()
        } else {
            t
        }
    };

    if is_auto && title_entry.text() != use_title.as_str() {
        title_auto_flag.set(true);
        title_entry.set_text(&use_title);
        title_auto_flag.set(false);
    }

    let note_id = {
        let mut s = session.borrow_mut();
        s.note_id.get_or_insert_with(new_note_id).clone()
    };

    let now = now_iso8601();
    let note = InboxNote {
        id: note_id.clone(),
        title: use_title,
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

    let result = db.borrow().as_ref().map(|db| db.upsert_note(&note));

    match result {
        Some(Ok(())) => {
            let mut s = session.borrow_mut();
            s.note_id = Some(note_id);
            s.auto_titled = is_auto;
            s.dirty = false;
            pending_timer.borrow_mut().take();
            save_label.set_text("Saved");
        }
        Some(Err(e)) => {
            eprintln!("blot: autosave error: {e}");
            save_label.set_text("Save error");
        }
        None => {
            save_label.set_text("DB unavailable");
        }
    }
}
