//! Merge Notes dialog: lets the user pick inbox notes to merge into the current note.

use crate::inbox::{format_date_short, InboxDb, InboxNote};
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// Open the Merge Notes dialog for an Inbox note.
/// The user selects source notes to merge into `target_note_id`.
/// `on_merge(source_ids)` is called with the IDs of notes to merge in.
pub fn open_inbox(
    parent: &gtk::ApplicationWindow,
    db: Rc<RefCell<Option<InboxDb>>>,
    target_note_id: &str,
    on_merge: impl Fn(Vec<String>) + 'static,
) {
    let notes: Vec<InboxNote> = db
        .borrow()
        .as_ref()
        .and_then(|d| d.list_notes().ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|n| n.id != target_note_id)
        .collect();

    let dialog = gtk::Window::builder()
        .transient_for(parent)
        .modal(true)
        .title("Merge Notes")
        .default_width(480)
        .default_height(400)
        .resizable(true)
        .build();
    dialog.add_css_class("merge-dialog-window");

    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);

    let hint = gtk::Label::new(Some(
        "Select notes to merge into the current note. Selected notes will be archived.",
    ));
    hint.add_css_class("merge-hint");
    hint.set_margin_top(14);
    hint.set_margin_bottom(8);
    hint.set_margin_start(14);
    hint.set_margin_end(14);
    hint.set_wrap(true);
    hint.set_halign(gtk::Align::Start);

    let sep = gtk::Separator::new(gtk::Orientation::Horizontal);

    let scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();

    let list = gtk::ListBox::new();
    list.add_css_class("merge-note-list");
    list.set_selection_mode(gtk::SelectionMode::Multiple);

    let checked: Rc<RefCell<std::collections::HashSet<String>>> =
        Rc::new(RefCell::new(std::collections::HashSet::new()));

    if notes.is_empty() {
        let row = gtk::ListBoxRow::new();
        let lbl = gtk::Label::builder()
            .label("No other inbox notes available.")
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .build();
        row.set_child(Some(&lbl));
        list.append(&row);
    }

    for note in &notes {
        let row = gtk::ListBoxRow::new();
        row.add_css_class("merge-note-row");
        row.set_widget_name(&note.id);

        let hbox = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        hbox.set_margin_top(8);
        hbox.set_margin_bottom(8);
        hbox.set_margin_start(12);
        hbox.set_margin_end(12);

        let check = gtk::CheckButton::new();
        let note_id_c = note.id.clone();
        let checked_c = checked.clone();
        check.connect_toggled(move |btn| {
            if btn.is_active() {
                checked_c.borrow_mut().insert(note_id_c.clone());
            } else {
                checked_c.borrow_mut().remove(&note_id_c);
            }
        });

        let text_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text_box.set_hexpand(true);

        let title_lbl = gtk::Label::new(Some(&note.title));
        title_lbl.add_css_class("merge-note-title");
        title_lbl.set_halign(gtk::Align::Start);
        title_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let date_lbl = gtk::Label::new(Some(&format!(
            "Updated {}",
            format_date_short(&note.updated_at)
        )));
        date_lbl.add_css_class("merge-note-date");
        date_lbl.set_halign(gtk::Align::Start);

        text_box.append(&title_lbl);
        text_box.append(&date_lbl);

        hbox.append(&check);
        hbox.append(&text_box);
        row.set_child(Some(&hbox));
        list.append(&row);
    }

    scroll.set_child(Some(&list));

    let btn_bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    btn_bar.set_halign(gtk::Align::End);
    btn_bar.set_margin_top(8);
    btn_bar.set_margin_bottom(12);
    btn_bar.set_margin_end(12);

    let cancel_btn = gtk::Button::with_label("Cancel");
    let merge_btn = gtk::Button::with_label("Merge");
    merge_btn.add_css_class("suggested-action");

    btn_bar.append(&cancel_btn);
    btn_bar.append(&merge_btn);

    vbox.append(&hint);
    vbox.append(&sep);
    vbox.append(&scroll);
    vbox.append(&btn_bar);
    dialog.set_child(Some(&vbox));

    let dialog_c = dialog.clone();
    cancel_btn.connect_clicked(move |_| dialog_c.close());

    let dialog_m = dialog.clone();
    merge_btn.connect_clicked(move |_| {
        let ids: Vec<String> = checked.borrow().iter().cloned().collect();
        if !ids.is_empty() {
            on_merge(ids);
        }
        dialog_m.close();
    });

    dialog.present();
}
