//! Direct JSON `.water` workspace UI for Watercolor v0.1 files.
//!
//! This shell is intentionally small: it opens the current JSON `.water`
//! format directly, lists note objects, edits note title/body, and writes the
//! same file back through the safe writer in `water_file`.

use crate::water_file::{
    note_body, note_objects, parse_water_file, save_water_file, update_note_body, WaterFileError,
    WaterWorkspace,
};
use gtk::prelude::*;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[derive(Clone)]
pub struct WaterWorkspaceShell {
    pub root: gtk::Box,
    workspace_label: gtk::Label,
    path_label: gtk::Label,
    note_list: gtk::ListBox,
    title_entry: gtk::Entry,
    body_view: gtk::TextView,
    save_button: gtk::Button,
    message_label: gtk::Label,
    status_save_label: gtk::Label,
    status_location_label: gtk::Label,
    state: Rc<RefCell<Option<WaterWorkspaceState>>>,
    loading: Rc<RefCell<bool>>,
}

#[derive(Clone)]
struct WaterWorkspaceState {
    path: PathBuf,
    workspace: WaterWorkspace,
    current_note_id: Option<String>,
    dirty: bool,
}

impl WaterWorkspaceShell {
    pub fn new(status_save_label: gtk::Label, status_location_label: gtk::Label) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("workspace-shell");

        let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 0);
        sidebar.add_css_class("workspace-sidebar");
        sidebar.set_size_request(280, -1);

        let header = gtk::Box::new(gtk::Orientation::Vertical, 2);
        header.add_css_class("workspace-sidebar-header");
        header.set_margin_start(12);
        header.set_margin_end(8);
        header.set_margin_top(12);
        header.set_margin_bottom(8);

        let workspace_label = gtk::Label::new(Some("No workspace"));
        workspace_label.add_css_class("workspace-name-label");
        workspace_label.set_halign(gtk::Align::Start);
        workspace_label.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let path_label = gtk::Label::new(None);
        path_label.add_css_class("workspace-section-label");
        path_label.set_halign(gtk::Align::Start);
        path_label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);

        header.append(&workspace_label);
        header.append(&path_label);
        sidebar.append(&header);

        let notes_label = gtk::Label::new(Some("Notes"));
        notes_label.add_css_class("workspace-section-label");
        notes_label.set_halign(gtk::Align::Start);
        notes_label.set_margin_start(12);
        notes_label.set_margin_end(8);
        notes_label.set_margin_top(4);
        notes_label.set_margin_bottom(2);
        sidebar.append(&notes_label);

        let note_scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let note_list = gtk::ListBox::new();
        note_list.add_css_class("workspace-room-list");
        note_list.set_selection_mode(gtk::SelectionMode::Single);
        note_scroll.set_child(Some(&note_list));
        sidebar.append(&note_scroll);

        root.append(&sidebar);
        root.append(&gtk::Separator::new(gtk::Orientation::Vertical));

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
        editor_outer.set_size_request(680, -1);

        let toolbar_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        toolbar_row.set_margin_bottom(8);

        let breadcrumb = gtk::Label::new(Some("Workspace note"));
        breadcrumb.add_css_class("editor-breadcrumb");
        breadcrumb.set_halign(gtk::Align::Start);
        breadcrumb.set_hexpand(true);

        let save_button = gtk::Button::with_label("Save");
        save_button.add_css_class("mode-button");
        save_button.set_sensitive(false);
        save_button.set_tooltip_text(Some("Save this .water file"));

        toolbar_row.append(&breadcrumb);
        toolbar_row.append(&save_button);

        let title_entry = gtk::Entry::new();
        title_entry.add_css_class("note-title-entry");
        title_entry.set_placeholder_text(Some("Select a note"));
        title_entry.set_has_frame(false);
        title_entry.set_margin_bottom(8);
        title_entry.set_sensitive(false);

        let body_view = gtk::TextView::new();
        body_view.add_css_class("note-body-view");
        body_view.set_wrap_mode(gtk::WrapMode::WordChar);
        body_view.set_top_margin(4);
        body_view.set_left_margin(2);
        body_view.set_right_margin(2);
        body_view.set_bottom_margin(4);
        body_view.set_vexpand(true);
        body_view.set_hexpand(true);
        body_view.set_sensitive(false);

        let message_label = gtk::Label::new(Some("Open a .water file to edit note objects."));
        message_label.add_css_class("editor-hint");
        message_label.set_halign(gtk::Align::Start);
        message_label.set_margin_top(8);

        editor_outer.append(&toolbar_row);
        editor_outer.append(&title_entry);
        editor_outer.append(&body_view);
        editor_outer.append(&message_label);
        editor_scroll.set_child(Some(&editor_outer));
        editor_panel.append(&editor_scroll);
        root.append(&editor_panel);

        let shell = Self {
            root,
            workspace_label,
            path_label,
            note_list: note_list.clone(),
            title_entry: title_entry.clone(),
            body_view: body_view.clone(),
            save_button: save_button.clone(),
            message_label,
            status_save_label,
            status_location_label,
            state: Rc::new(RefCell::new(None)),
            loading: Rc::new(RefCell::new(false)),
        };

        {
            let shell = shell.clone();
            note_list.connect_row_activated(move |_, row| {
                shell.open_note(&row.widget_name());
            });
        }

        {
            let shell = shell.clone();
            title_entry.connect_changed(move |_| {
                shell.mark_dirty();
            });
        }

        {
            let shell = shell.clone();
            body_view.buffer().connect_changed(move |_| {
                shell.mark_dirty();
            });
        }

        {
            let shell = shell.clone();
            save_button.connect_clicked(move |_| {
                shell.force_save_sync();
            });
        }

        shell
    }

    pub fn open_path(&self, path: &Path) -> Result<String, WaterFileError> {
        let workspace = parse_water_file(path)?;
        let name = workspace.workspace_name.clone();
        *self.state.borrow_mut() = Some(WaterWorkspaceState {
            path: path.to_path_buf(),
            workspace,
            current_note_id: None,
            dirty: false,
        });

        self.refresh();
        Ok(name)
    }

    pub fn refresh(&self) {
        self.clear_note_list();
        let Some(state) = self.state.borrow().clone() else {
            self.workspace_label.set_text("No workspace");
            self.path_label.set_text("");
            self.clear_editor("Open a .water file to edit note objects.");
            return;
        };

        self.workspace_label
            .set_text(&state.workspace.workspace_name);
        self.status_location_label
            .set_text(&state.workspace.workspace_name);
        self.path_label.set_text(&state.path.display().to_string());

        let notes = note_objects(&state.workspace);
        for note in notes {
            self.append_note_row(&note.object_id, &note.title);
        }

        if let Some(note_id) = state.current_note_id {
            self.open_note(&note_id);
        } else if let Some(first) = note_objects(&state.workspace).first() {
            self.open_note(&first.object_id);
        } else {
            self.clear_editor("No note objects in this .water file.");
        }
    }

    pub fn force_save_sync(&self) {
        if let Err(error) = self.save_current_note_to_state() {
            self.set_save_error(&error.to_string());
            return;
        }

        let Some(mut state) = self.state.borrow().clone() else {
            return;
        };
        if !state.dirty {
            return;
        }

        match save_water_file(&state.path, &state.workspace) {
            Ok(backup) => {
                state.dirty = false;
                *self.state.borrow_mut() = Some(state);
                self.save_button.set_sensitive(false);
                self.message_label
                    .set_text(&format!("Saved. Backup: {}", backup.display()));
                super::set_save_status(&self.status_save_label, "Saved");
            }
            Err(error) => {
                self.set_save_error(&error.to_string());
            }
        }
    }

    fn open_note(&self, note_id: &str) {
        if let Err(error) = self.save_current_note_to_state() {
            self.set_save_error(&error.to_string());
            return;
        }

        let Some(mut state) = self.state.borrow().clone() else {
            return;
        };
        let Some(note) = state
            .workspace
            .objects
            .iter()
            .find(|object| object.object_id == note_id && object.object_type == "note")
        else {
            self.clear_editor("Note object not found.");
            return;
        };

        state.current_note_id = Some(note.object_id.clone());
        *self.state.borrow_mut() = Some(state.clone());

        *self.loading.borrow_mut() = true;
        self.title_entry.set_sensitive(true);
        self.body_view.set_sensitive(true);
        self.title_entry.set_text(&note.title);
        let buffer = self.body_view.buffer();
        buffer.set_text(note_body(note).unwrap_or(""));
        self.message_label
            .set_text("Editing direct .water note body.");
        super::set_save_status(
            &self.status_save_label,
            if state.dirty { "Unsaved" } else { "Opened" },
        );
        self.save_button.set_sensitive(state.dirty);
        *self.loading.borrow_mut() = false;
    }

    fn save_current_note_to_state(&self) -> Result<(), WaterFileError> {
        if *self.loading.borrow() {
            return Ok(());
        }

        let Some(mut state) = self.state.borrow().clone() else {
            return Ok(());
        };
        let Some(note_id) = state.current_note_id.clone() else {
            return Ok(());
        };

        let title = self.title_entry.text().to_string();
        let buffer = self.body_view.buffer();
        let body = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .to_string();

        let Some(note) = state
            .workspace
            .objects
            .iter_mut()
            .find(|object| object.object_id == note_id)
        else {
            return Err(WaterFileError::Validation(format!(
                "note object {note_id} not found"
            )));
        };

        if note.title != title {
            note.title = title;
            state.dirty = true;
        }
        let before = note_body(note).unwrap_or("").to_string();
        if before != body {
            update_note_body(&mut state.workspace, &note_id, body)?;
            state.dirty = true;
        }

        *self.state.borrow_mut() = Some(state);
        Ok(())
    }

    fn mark_dirty(&self) {
        if *self.loading.borrow() {
            return;
        }
        if let Some(state) = self.state.borrow_mut().as_mut() {
            state.dirty = true;
            self.save_button.set_sensitive(true);
            self.message_label.set_text("Unsaved changes.");
            super::set_save_status(&self.status_save_label, "Unsaved");
        }
    }

    fn clear_editor(&self, message: &str) {
        *self.loading.borrow_mut() = true;
        self.title_entry.set_text("");
        self.title_entry.set_sensitive(false);
        self.body_view.buffer().set_text("");
        self.body_view.set_sensitive(false);
        self.save_button.set_sensitive(false);
        self.message_label.set_text(message);
        *self.loading.borrow_mut() = false;
    }

    fn set_save_error(&self, message: &str) {
        self.message_label.set_text(message);
        super::set_save_status(&self.status_save_label, "Save error");
        self.save_button.set_sensitive(true);
        eprintln!("blot: .water save error: {message}");
    }

    fn clear_note_list(&self) {
        while let Some(child) = self.note_list.first_child() {
            self.note_list.remove(&child);
        }
    }

    fn append_note_row(&self, note_id: &str, title: &str) {
        let row = gtk::ListBoxRow::new();
        row.set_widget_name(note_id);

        let label = gtk::Label::new(Some(if title.trim().is_empty() {
            "Untitled note"
        } else {
            title
        }));
        label.set_halign(gtk::Align::Start);
        label.set_margin_start(12);
        label.set_margin_end(8);
        label.set_margin_top(6);
        label.set_margin_bottom(6);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);

        row.set_child(Some(&label));
        self.note_list.append(&row);
    }
}
