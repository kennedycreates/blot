//! External File Mode — edit a plain `.txt` / `.md` / `.markdown` file directly.
//!
//! An external file is a distinct note kind: it is **not** an Inbox note and
//! **not** a workspace note. It does not autosave to disk (manual Save only)
//! and it does not enter the Blot search index until absorbed. A banner offers
//! to absorb it into Blot.
//!
//! Safety: saving warns if the file changed on disk since it was opened, never
//! autosaves to disk, and on save failure keeps the editor content.

use super::modal_host::ModalHost;
use crate::external_file::{self, ExternalFile};
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Callback invoked when the user asks to absorb the current external file.
/// Receives the loaded file and the current (possibly user-edited) title.
pub type AbsorbCb = Rc<RefCell<Option<Box<dyn Fn(ExternalFile, String)>>>>;

#[derive(Clone)]
pub struct ExternalFileShell {
    pub root: gtk::ScrolledWindow,
    pub title_entry: gtk::Entry,
    pub body_view: gtk::TextView,
    /// The currently open external file, if any.
    pub current: Rc<RefCell<Option<ExternalFile>>>,
    /// True when the body has unsaved edits relative to disk.
    pub dirty: Rc<Cell<bool>>,
    /// Suppresses dirty-tracking while we load content programmatically.
    loading_flag: Rc<Cell<bool>>,
    banner: gtk::Box,
    save_label: gtk::Label,
    location_label: gtk::Label,
    /// In-window modal host for save prompts and errors.
    modal_host: ModalHost,
    /// Installed by main_window: opens the absorb dialog for the current file.
    pub on_absorb: AbsorbCb,
}

impl ExternalFileShell {
    /// Open and load an external file. Returns an error message on failure
    /// (the caller should show it and keep the user where they were).
    pub fn open_path(&self, path: &std::path::Path) -> Result<(), String> {
        let ef = external_file::read_external_file(path).map_err(|e| e.to_string())?;
        self.load(ef);
        Ok(())
    }

    /// Load an already-read external file into the editor surface.
    pub fn load(&self, ef: ExternalFile) {
        self.loading_flag.set(true);
        self.title_entry.set_text(&ef.initial_title());
        self.body_view.buffer().set_text(&ef.content);
        self.loading_flag.set(false);

        let kind_label = ef.kind.label();
        self.location_label.set_text("External File");
        super::set_save_status(&self.save_label, "External file — manual save");
        self.banner_message(&format!(
            "This is a {}. Editing saves to the file only when you choose Save.",
            kind_label.to_lowercase()
        ));
        self.banner.set_visible(true);

        *self.current.borrow_mut() = Some(ef);
        self.dirty.set(false);
        self.body_view.grab_focus();
    }

    /// Current editor body text.
    fn body_text(&self) -> String {
        let buf = self.body_view.buffer();
        buf.text(&buf.start_iter(), &buf.end_iter(), false)
            .to_string()
    }

    /// Save the editor content back to the original file path. Warns (via an
    /// in-window confirm) if the file changed on disk since it was opened.
    pub fn save(&self) {
        let ef = match self.current.borrow().clone() {
            Some(ef) => ef,
            None => return,
        };
        let changed = external_file::file_changed_since_open(&ef.path, ef.mtime_snapshot);
        if changed {
            // Warn before overwriting external changes.
            let shell = self.clone();
            let ef2 = ef.clone();
            self.modal_host.show_confirm(
                "File changed on disk",
                "This file was modified by another program since you opened it. \
                 Saving will overwrite those changes.",
                "Overwrite",
                true,
                false,
                move || shell.write_now(&ef2),
            );
        } else {
            self.write_now(&ef);
        }
    }

    fn write_now(&self, ef: &ExternalFile) {
        let text = self.body_text();
        match external_file::save_external_file(
            &ef.path,
            &text,
            ef.line_ending,
            ef.had_trailing_newline,
        ) {
            Ok(()) => {
                self.dirty.set(false);
                super::set_save_status(&self.save_label, "Saved to file");
                // Refresh the mtime snapshot so we don't falsely warn next save.
                if let Ok(updated) = external_file::read_external_file(&ef.path) {
                    if let Some(cur) = self.current.borrow_mut().as_mut() {
                        cur.mtime_snapshot = updated.mtime_snapshot;
                        cur.original_modified_at = updated.original_modified_at;
                        cur.line_ending = updated.line_ending;
                    }
                }
            }
            Err(e) => {
                eprintln!("blot: external file save error: {e}");
                super::set_save_status(&self.save_label, "Save error — content kept");
                self.modal_host
                    .show_error("Could not save file", &format!("{e}"));
            }
        }
    }

    /// Trigger the absorb flow for the current file (banner button / command).
    ///
    /// The suggested title prefers an explicit edit the user made to the title
    /// field; otherwise it derives a smart title from the current body content
    /// (heading → file stem → first line → timestamp).
    pub fn trigger_absorb(&self) {
        let current = self.current.borrow().clone();
        if let Some(ef) = current {
            let entry_text = self.title_entry.text().to_string();
            let suggested = if !entry_text.trim().is_empty() && entry_text != ef.initial_title() {
                entry_text
            } else {
                let now = crate::inbox::now_iso8601();
                let ts = crate::inbox::format_date_short(&now).to_string();
                external_file::derive_file_title(&self.body_text(), &ef.stem, &ts)
            };
            if let Some(f) = self.on_absorb.borrow().as_ref() {
                f(ef, suggested);
            }
        }
    }

    /// True when an external file is currently open with unsaved edits.
    pub fn has_unsaved_changes(&self) -> bool {
        self.current.borrow().is_some() && self.dirty.get()
    }

    /// Write to disk without the changed-on-disk prompt. Used by the
    /// window-close confirmation path where an async dialog isn't possible.
    pub fn save_sync(&self) {
        if let Some(ef) = self.current.borrow().clone() {
            self.write_now(&ef);
        }
    }

    fn banner_message(&self, msg: &str) {
        if let Some(lbl) = self
            .banner
            .first_child()
            .and_then(|w| w.downcast::<gtk::Label>().ok())
        {
            lbl.set_text(msg);
        }
    }
}

/// Build the External File shell.
pub fn build(
    save_label: gtk::Label,
    location_label: gtk::Label,
    modal_host: ModalHost,
) -> ExternalFileShell {
    let loading_flag: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let dirty: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let current: Rc<RefCell<Option<ExternalFile>>> = Rc::new(RefCell::new(None));
    let on_absorb: AbsorbCb = Rc::new(RefCell::new(None));

    let scrolled = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hexpand(true)
        .build();
    scrolled.add_css_class("editor-scroll");

    let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    outer.set_halign(gtk::Align::Center);
    outer.set_hexpand(true);
    outer.set_margin_top(36);
    outer.set_margin_bottom(64);
    outer.set_margin_start(24);
    outer.set_margin_end(24);
    outer.set_size_request(640, -1);

    // ── Absorb banner ──────────────────────────────────────────────────────
    let banner = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    banner.add_css_class("external-file-banner");
    banner.set_margin_bottom(12);
    banner.set_visible(false);

    let banner_label = gtk::Label::new(Some("This is a plain text file."));
    banner_label.add_css_class("external-file-banner-label");
    banner_label.set_halign(gtk::Align::Start);
    banner_label.set_hexpand(true);
    banner_label.set_wrap(true);

    let keep_btn = gtk::Button::with_label("Keep Editing as File");
    keep_btn.add_css_class("editor-action-btn");
    let absorb_btn = gtk::Button::with_label("Absorb into Blot");
    absorb_btn.add_css_class("place-note-btn");
    let dismiss_btn = gtk::Button::with_label("Dismiss");
    dismiss_btn.add_css_class("editor-action-btn");

    banner.append(&banner_label);
    banner.append(&keep_btn);
    banner.append(&absorb_btn);
    banner.append(&dismiss_btn);

    // ── Toolbar (location + Save) ──────────────────────────────────────────
    let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    toolbar.set_margin_bottom(8);

    let breadcrumb = gtk::Label::new(Some("External File"));
    breadcrumb.add_css_class("editor-breadcrumb");
    breadcrumb.set_halign(gtk::Align::Start);
    breadcrumb.set_hexpand(true);

    let save_btn = gtk::Button::with_label("Save");
    save_btn.add_css_class("editor-action-btn");
    save_btn.set_tooltip_text(Some("Save changes back to the file on disk"));

    let absorb_btn2 = gtk::Button::with_label("Absorb into Blot");
    absorb_btn2.add_css_class("place-note-btn");
    absorb_btn2.set_tooltip_text(Some("Create a Blot note from this file"));

    toolbar.append(&breadcrumb);
    toolbar.append(&save_btn);
    toolbar.append(&absorb_btn2);

    // ── Title + body ──────────────────────────────────────────────────────
    let title_entry = gtk::Entry::new();
    title_entry.add_css_class("note-title-entry");
    title_entry.set_placeholder_text(Some("File name"));
    title_entry.set_has_frame(false);
    title_entry.set_margin_bottom(8);

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

    let hint = gtk::Label::new(Some(
        "External file — manual save. Use Absorb into Blot to bring it into your notes.",
    ));
    hint.add_css_class("editor-hint");
    hint.set_halign(gtk::Align::Start);
    hint.set_margin_top(8);

    outer.append(&banner);
    outer.append(&toolbar);
    outer.append(&title_entry);
    outer.append(&body_view);
    outer.append(&hint);
    scrolled.set_child(Some(&outer));

    let shell = ExternalFileShell {
        root: scrolled,
        title_entry: title_entry.clone(),
        body_view: body_view.clone(),
        current: current.clone(),
        dirty: dirty.clone(),
        loading_flag: loading_flag.clone(),
        banner: banner.clone(),
        save_label: save_label.clone(),
        location_label: location_label.clone(),
        modal_host,
        on_absorb: on_absorb.clone(),
    };

    // Body changes mark dirty (no disk autosave).
    {
        let dirty = dirty.clone();
        let loading_flag = loading_flag.clone();
        let save_label = save_label.clone();
        body_view.buffer().connect_changed(move |_| {
            if loading_flag.get() {
                return;
            }
            dirty.set(true);
            super::set_save_status(&save_label, "External file — unsaved");
        });
    }

    // Banner buttons.
    {
        let banner = banner.clone();
        keep_btn.connect_clicked(move |_| banner.set_visible(false));
    }
    {
        let banner = banner.clone();
        dismiss_btn.connect_clicked(move |_| banner.set_visible(false));
    }
    {
        let shell = shell.clone();
        absorb_btn.connect_clicked(move |_| shell.trigger_absorb());
    }
    {
        let shell = shell.clone();
        absorb_btn2.connect_clicked(move |_| shell.trigger_absorb());
    }

    // Save button → write back to disk.
    {
        let shell = shell.clone();
        save_btn.connect_clicked(move |_| shell.save());
    }

    shell
}
