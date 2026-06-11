//! Absorb-into-Blot dialog: choose a destination + title for an external file,
//! decide what happens to the original, then create the Blot note.
//!
//! Destinations offered:
//! - Global Inbox (default when no workspace is focused)
//! - Current Workspace → Loose Notes (when a workspace is open; suggested then)
//!
//! Absorbing into a Shelf/Pile is supported by the absorb API and exercised by
//! tests; the in-UI picker offers the common Inbox / Loose Notes choices here
//! (a full Room/Shelf/Pile picker is a Prompt 12 follow-up — see TODOs).
//!
//! Original-file action (explicit user choice, never automatic):
//! - Leave It Where It Is (safe default)
//! - Move to Trash (GIO/GVfs trash, never permanent delete)

use super::modal_host::{self, ButtonKind, ModalHost};
use crate::absorb::{self, AbsorbResult, OriginalAction};
use crate::external_file::{self, ExternalFile};
use crate::inbox::InboxDb;
use crate::place_note::PlaceDestination;
use crate::workspace::WorkspaceDb;
use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// Show the absorb dialog. `on_done` runs after a successful absorb with the
/// result so the caller can open the new note.
pub fn show(
    host: &ModalHost,
    ef: ExternalFile,
    suggested_title: String,
    inbox_db: Rc<RefCell<Option<InboxDb>>>,
    workspace_db: Rc<RefCell<Option<WorkspaceDb>>>,
    on_done: impl Fn(AbsorbResult) + 'static,
) {
    let on_done: Rc<dyn Fn(AbsorbResult)> = Rc::new(on_done);

    // Determine whether a workspace is open and its default room.
    let workspace_info: Option<(String, String)> = workspace_db.borrow().as_ref().and_then(|ws| {
        ws.default_room_id()
            .map(|room_id| (ws.workspace_name(), room_id))
    });

    // Duplicate detection.
    let already_absorbed = {
        let guard = inbox_db.borrow();
        guard
            .as_ref()
            .map(|db| absorb::was_absorbed_before(db, &ef.path))
            .unwrap_or(false)
    };

    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 10);
    vbox.add_css_class("absorb-dialog");
    vbox.set_size_request(440, -1);

    let heading = gtk::Label::new(Some(&format!("Absorb “{}” into Blot", ef.original_name)));
    heading.add_css_class("absorb-heading");
    heading.set_halign(gtk::Align::Start);
    heading.set_wrap(true);
    vbox.append(&heading);

    if already_absorbed {
        let warn = gtk::Label::new(Some(
            "This file appears to have been absorbed before. You can continue \
             — a new, separate note will be created.",
        ));
        warn.add_css_class("absorb-warning");
        warn.set_halign(gtk::Align::Start);
        warn.set_wrap(true);
        vbox.append(&warn);
    }

    // ── Title ──────────────────────────────────────────────────────────────
    let title_lbl = gtk::Label::new(Some("Title"));
    title_lbl.set_halign(gtk::Align::Start);
    title_lbl.add_css_class("absorb-section-label");
    vbox.append(&title_lbl);

    let title_entry = gtk::Entry::new();
    title_entry.set_text(&suggested_title);
    vbox.append(&title_entry);

    // ── Destination ──────────────────────────────────────────────────────────
    let dest_lbl = gtk::Label::new(Some("Destination"));
    dest_lbl.set_halign(gtk::Align::Start);
    dest_lbl.set_margin_top(6);
    dest_lbl.add_css_class("absorb-section-label");
    vbox.append(&dest_lbl);

    let inbox_radio = gtk::CheckButton::with_label("Global Inbox");
    vbox.append(&inbox_radio);

    let ws_radio = if let Some((ws_name, _)) = &workspace_info {
        let r = gtk::CheckButton::with_label(&format!("{ws_name} → Loose Notes"));
        r.set_group(Some(&inbox_radio));
        vbox.append(&r);
        Some(r)
    } else {
        None
    };

    // Suggest workspace when one is focused, otherwise Inbox.
    match &ws_radio {
        Some(r) => r.set_active(true),
        None => inbox_radio.set_active(true),
    }

    // ── Original file ──────────────────────────────────────────────────────────
    let orig_lbl = gtk::Label::new(Some("Original file"));
    orig_lbl.set_halign(gtk::Align::Start);
    orig_lbl.set_margin_top(6);
    orig_lbl.add_css_class("absorb-section-label");
    vbox.append(&orig_lbl);

    let leave_radio = gtk::CheckButton::with_label("Leave It Where It Is");
    leave_radio.set_active(true);
    vbox.append(&leave_radio);

    let trash_radio = gtk::CheckButton::with_label("Move to Trash");
    trash_radio.set_group(Some(&leave_radio));
    vbox.append(&trash_radio);

    // ── Buttons ──────────────────────────────────────────────────────────────
    let actions = modal_host::build_modal_actions();
    let cancel_btn = modal_host::build_modal_button("Cancel", ButtonKind::Secondary, {
        let host = host.clone();
        move || host.hide()
    });
    let absorb_btn = modal_host::build_modal_button("Absorb", ButtonKind::Primary, || {});
    actions.append(&cancel_btn);
    actions.append(&absorb_btn);

    {
        let host = host.clone();
        let inbox_db = inbox_db.clone();
        let workspace_db = workspace_db.clone();
        let on_done = on_done.clone();
        let title_entry = title_entry.clone();
        let trash_radio = trash_radio.clone();
        let ws_radio = ws_radio.clone();
        let workspace_info = workspace_info.clone();
        let ef = ef.clone();

        absorb_btn.connect_clicked(move |_| {
            let title = {
                let t = title_entry.text().to_string();
                if t.trim().is_empty() {
                    "Untitled file".to_string()
                } else {
                    t
                }
            };
            let action = if trash_radio.is_active() {
                OriginalAction::Trash
            } else {
                OriginalAction::Leave
            };
            let to_workspace = ws_radio.as_ref().map(|r| r.is_active()).unwrap_or(false);

            // Perform the absorb. The note is created and provenance recorded
            // before we touch the original file.
            let result: Result<AbsorbResult, String> = if to_workspace {
                let (_, room_id) = workspace_info.clone().expect("workspace info present");
                let inbox_guard = inbox_db.borrow();
                let ws_guard = workspace_db.borrow();
                match (inbox_guard.as_ref(), ws_guard.as_ref()) {
                    (Some(idb), Some(ws)) => absorb::absorb_into_workspace(
                        idb,
                        ws,
                        &ef,
                        &title,
                        &PlaceDestination::LooseInRoom { room_id },
                        action,
                    )
                    .map_err(|e| e.to_string()),
                    _ => Err("Databases unavailable.".to_string()),
                }
            } else {
                let inbox_guard = inbox_db.borrow();
                match inbox_guard.as_ref() {
                    Some(idb) => absorb::absorb_into_inbox(idb, &ef, &title, action)
                        .map_err(|e| e.to_string()),
                    None => Err("Inbox database unavailable.".to_string()),
                }
            };

            match result {
                Ok(absorb_result) => {
                    // Now perform the original-file action (explicit user choice).
                    let on_done = on_done.clone();
                    if action == OriginalAction::Trash {
                        if let Err(e) = external_file::move_to_trash(&ef.path) {
                            // Note is safe; only the trash failed. Open the new
                            // note, then surface the warning over it.
                            on_done(absorb_result);
                            host.show_error(
                                "Could not move file to Trash",
                                &format!(
                                    "{e}\n\nThe note was absorbed successfully; the original \
                                     file was left in place."
                                ),
                            );
                            return;
                        }
                    }
                    host.hide();
                    on_done(absorb_result);
                }
                Err(msg) => {
                    // Absorb failed: original file untouched, editor content kept.
                    host.show_error("Could not absorb file", &msg);
                }
            }
        });
    }

    host.show_with_custom_ui("Absorb into Blot", &vbox, &actions, true, None);
}
