//! Version history dialog for Blot notes.
//! Shows a list of snapshots (auto + bookmarks) with preview and restore/copy actions.

use crate::inbox::{format_date_short, InboxDb, InboxNote};
use crate::note_version::NoteVersion;
use crate::ops;
use crate::workspace::{WorkspaceDb, WorkspaceNote};
use super::modal_host::{self, ButtonKind, ModalHost};
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

// ── Source enum ───────────────────────────────────────────────────────────────

pub enum VersionSource {
    Inbox,
    Workspace,
}

// ── Public entry points ───────────────────────────────────────────────────────

/// Open the version history dialog for an Inbox note.
pub fn open_inbox(
    host: &ModalHost,
    db: Rc<RefCell<Option<InboxDb>>>,
    note_id: &str,
    on_restored: impl Fn(InboxNote) + 'static,
) {
    let versions: Vec<NoteVersion> = db
        .borrow()
        .as_ref()
        .and_then(|d| d.list_versions(note_id).ok())
        .unwrap_or_default();

    let note = db
        .borrow()
        .as_ref()
        .and_then(|d| d.get_note(note_id).ok().flatten());

    let Some(note) = note else {
        eprintln!("blot: version history: note {note_id} not found");
        return;
    };

    open_dialog(
        host,
        &note.title,
        versions,
        VersionSource::Inbox,
        move |version_id| {
            let Some(version) = db
                .borrow()
                .as_ref()
                .and_then(|d| d.get_version(&version_id).ok().flatten())
            else {
                return;
            };
            let db_ref = db.borrow();
            let Some(d) = db_ref.as_ref() else { return };
            match ops::restore_inbox_version(d, &version) {
                Ok(restored) => on_restored(restored),
                Err(e) => eprintln!("blot: restore error: {e}"),
            }
        },
    );
}

/// Open the version history dialog for a workspace note.
pub fn open_workspace(
    host: &ModalHost,
    db: Rc<RefCell<Option<WorkspaceDb>>>,
    note_id: &str,
    on_restored: impl Fn(WorkspaceNote) + 'static,
) {
    let versions: Vec<NoteVersion> = db
        .borrow()
        .as_ref()
        .and_then(|d| d.list_note_versions(note_id).ok())
        .unwrap_or_default();

    let note = db
        .borrow()
        .as_ref()
        .and_then(|d| d.get_note(note_id).ok().flatten());

    let Some(note) = note else {
        eprintln!("blot: version history: workspace note {note_id} not found");
        return;
    };

    open_dialog(
        host,
        &note.title,
        versions,
        VersionSource::Workspace,
        move |version_id| {
            let Some(version) = db
                .borrow()
                .as_ref()
                .and_then(|d| d.get_note_version(&version_id).ok().flatten())
            else {
                return;
            };
            let db_ref = db.borrow();
            let Some(d) = db_ref.as_ref() else { return };
            match ops::restore_workspace_version(d, &version) {
                Ok(restored) => on_restored(restored),
                Err(e) => eprintln!("blot: restore error: {e}"),
            }
        },
    );
}

// ── Bookmark name dialog ──────────────────────────────────────────────────────

/// Show a dialog asking for a bookmark name and call `on_confirmed(name)`.
pub fn prompt_bookmark_name(host: &ModalHost, on_confirmed: impl Fn(String) + 'static) {
    let on_confirmed = std::rc::Rc::new(on_confirmed);

    // Custom content (not show_input) because a blank name is allowed here —
    // it resolves to a timestamped default via `default_bookmark_name`.
    let content = gtk::Box::new(gtk::Orientation::Vertical, 10);
    content.add_css_class("modal-input-content");
    content.append(&modal_host::build_modal_prompt("Bookmark name:"));

    let entry = gtk::Entry::new();
    entry.add_css_class("dialog-entry");
    entry.set_placeholder_text(Some("e.g. Before major edit"));
    entry.set_hexpand(true);
    content.append(&entry);

    let actions = modal_host::build_modal_actions();

    let host_c = host.clone();
    let cancel_btn =
        modal_host::build_modal_button("Cancel", ButtonKind::Secondary, move || host_c.hide());
    actions.append(&cancel_btn);

    let host_s = host.clone();
    let entry_s = entry.clone();
    let on_confirmed_s = on_confirmed.clone();
    let save_btn = modal_host::build_modal_button("Bookmark", ButtonKind::Primary, move || {
        let name = default_bookmark_name(&entry_s.text());
        on_confirmed_s(name);
        host_s.hide();
    });
    actions.append(&save_btn);

    // Enter key in entry → activate Bookmark
    let save_for_key = save_btn.clone();
    entry.connect_activate(move |_| save_for_key.emit_clicked());

    host.show_with_custom_ui("Bookmark Version", &content, &actions, true, None);
    entry.grab_focus();
}

/// Resolve the bookmark name: trimmed user input, or a timestamped default
/// of the form "Bookmarked <local timestamp>" when blank.
fn default_bookmark_name(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        let now = crate::inbox::now_iso8601();
        format!("Bookmarked {}", now.replace('T', " "))
    } else {
        trimmed.to_string()
    }
}

// ── Internal dialog builder ───────────────────────────────────────────────────

fn open_dialog(
    host: &ModalHost,
    note_title: &str,
    versions: Vec<NoteVersion>,
    _source: VersionSource,
    on_restore: impl Fn(String) + 'static,
) {
    let title = format!("Version History — {note_title}");

    let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    paned.add_css_class("version-history-window");
    paned.set_position(260);
    // The host panel sizes to content, so give the panes explicit dimensions.
    paned.set_size_request(720, 460);

    // Left: list of versions
    let list_scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();

    let list = gtk::ListBox::new();
    list.add_css_class("version-list");
    list.set_selection_mode(gtk::SelectionMode::Browse);

    if versions.is_empty() {
        let row = gtk::ListBoxRow::new();
        let lbl = gtk::Label::builder()
            .label("No versions recorded yet.")
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        row.set_child(Some(&lbl));
        list.append(&row);
    }

    for v in &versions {
        let row = gtk::ListBoxRow::new();
        row.add_css_class("version-row");

        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 2);
        vbox.set_margin_top(8);
        vbox.set_margin_bottom(8);
        vbox.set_margin_start(12);
        vbox.set_margin_end(12);

        let badge_line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        if v.is_bookmark {
            let badge = gtk::Label::new(Some("★"));
            badge.add_css_class("version-bookmark-badge");
            badge_line.append(&badge);
        }
        let name = v
            .bookmark_name
            .as_deref()
            .unwrap_or_else(|| v.reason.as_str());
        let name_lbl = gtk::Label::new(Some(name));
        name_lbl.add_css_class("version-name");
        name_lbl.set_halign(gtk::Align::Start);
        name_lbl.set_hexpand(true);
        badge_line.append(&name_lbl);

        let date_lbl = gtk::Label::new(Some(format_date_short(&v.created_at)));
        date_lbl.add_css_class("version-date");
        date_lbl.set_halign(gtk::Align::End);

        vbox.append(&badge_line);
        vbox.append(&date_lbl);
        row.set_child(Some(&vbox));
        // Store the version id as widget name for retrieval on selection.
        row.set_widget_name(&v.id);
        list.append(&row);
    }

    list_scroll.set_child(Some(&list));

    // Right: preview area
    let preview_scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hexpand(true)
        .build();

    let preview_view = gtk::TextView::new();
    preview_view.add_css_class("version-preview");
    preview_view.set_wrap_mode(gtk::WrapMode::WordChar);
    preview_view.set_editable(false);
    preview_view.set_cursor_visible(false);
    preview_view.set_top_margin(12);
    preview_view.set_left_margin(16);
    preview_view.set_right_margin(16);
    preview_view.set_bottom_margin(12);
    preview_scroll.set_child(Some(&preview_view));

    paned.set_start_child(Some(&list_scroll));
    paned.set_end_child(Some(&preview_scroll));

    // Actions row (hosted by the modal): Copy / Restore / Close.
    let actions = modal_host::build_modal_actions();
    let copy_btn = modal_host::build_modal_button("Copy Body", ButtonKind::Secondary, || {});
    let restore_btn = modal_host::build_modal_button("Restore", ButtonKind::Primary, || {});
    restore_btn.set_sensitive(false);
    let host_close = host.clone();
    let close_btn =
        modal_host::build_modal_button("Close", ButtonKind::Secondary, move || host_close.hide());
    actions.append(&close_btn);
    actions.append(&copy_btn);
    actions.append(&restore_btn);

    // Populate preview on row selection
    let versions_rc: Rc<Vec<NoteVersion>> = Rc::new(versions);
    let versions_for_sel = versions_rc.clone();
    let preview_for_sel = preview_view.clone();
    let restore_btn_c = restore_btn.clone();
    list.connect_row_selected(move |_, row| {
        if let Some(row) = row {
            let vid = row.widget_name().to_string();
            if let Some(v) = versions_for_sel.iter().find(|v| v.id == vid) {
                preview_for_sel
                    .buffer()
                    .set_text(&format!("{}\n\n{}", v.title, v.body));
                restore_btn_c.set_sensitive(true);
            }
        }
    });

    // Copy body
    let versions_for_copy = versions_rc.clone();
    let list_for_copy = list.clone();
    copy_btn.connect_clicked(move |_| {
        let Some(row) = list_for_copy.selected_row() else {
            return;
        };
        let vid = row.widget_name().to_string();
        if let Some(v) = versions_for_copy.iter().find(|v| v.id == vid) {
            if let Some(display) = gtk::gdk::Display::default() {
                display.clipboard().set_text(&v.body);
            }
        }
    });

    // Restore
    let versions_for_restore = versions_rc.clone();
    let list_for_restore = list.clone();
    let host_for_restore = host.clone();
    restore_btn.connect_clicked(move |_| {
        let Some(row) = list_for_restore.selected_row() else {
            return;
        };
        let vid = row.widget_name().to_string();
        if versions_for_restore.iter().any(|v| v.id == vid) {
            on_restore(vid);
            host_for_restore.hide();
        }
    });

    // Select first row
    if let Some(first) = list.row_at_index(0) {
        list.select_row(Some(&first));
    }

    host.show_with_custom_ui(&title, &paned, &actions, true, None);
}
