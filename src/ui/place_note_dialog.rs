//! Place Note picker dialog.
//!
//! Modal window that lets the user choose a destination workspace, room,
//! and container (Loose Notes / Shelf / Pile) for an Inbox note.
//!
//! Inline creation of Rooms, Shelves, and Piles is supported — the user does
//! not need to leave the dialog to create a destination.
//!
//! On successful placement, the `on_placed` callback fires with the result.
//! The dialog closes itself on placement or cancellation.

use crate::inbox::InboxDb;
use crate::known_workspaces::KnownWorkspaceRegistry;
use crate::place_note::{
    compute_suggestion, place_inbox_note, record_last_used_destination, PlaceDestination,
    PlacementRequest,
};
use super::modal_host::{self, ButtonKind, ModalHost};
use crate::workspace::{ContainerKind, WorkspaceDb};
use gtk::prelude::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

// ── Dialog state ──────────────────────────────────────────────────────────────

/// Mutable state shared across all signal handlers in the dialog.
struct DialogState {
    /// Workspace paths in the same order as the workspace ComboBoxText.
    workspace_paths: Vec<PathBuf>,
    /// Room IDs in the same order as the room ComboBoxText.
    room_ids: Vec<String>,
    /// Container IDs (shelves + piles) in same order as container ComboBoxText.
    container_ids: Vec<String>,
    /// Container kinds parallel to container_ids.
    container_kinds: Vec<ContainerKind>,
}

impl DialogState {
    fn selected_workspace_path(&self, idx: u32) -> Option<&PathBuf> {
        self.workspace_paths.get(idx as usize)
    }

    fn selected_room_id(&self, idx: u32) -> Option<&str> {
        self.room_ids.get(idx as usize).map(|s| s.as_str())
    }

    fn selected_container_id(&self, idx: u32) -> Option<&str> {
        self.container_ids.get(idx as usize).map(|s| s.as_str())
    }

    fn selected_container_kind(&self, idx: u32) -> Option<&ContainerKind> {
        self.container_kinds.get(idx as usize)
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Open the Place Note dialog.
///
/// # Parameters
/// - `host` — the in-window modal host the picker is shown on.
/// - `inbox_note_id` — which Inbox note to place.
/// - `inbox_note_title` — displayed at the top of the dialog.
/// - `inbox_db` — shared access to the Inbox database.
/// - `workspace_db` — currently focused workspace (used for suggestion).
/// - `known_ws` — registry of known workspaces (populates workspace list).
/// - `on_placed` — called after successful placement with the result.
#[allow(clippy::too_many_arguments)]
pub fn show(
    host: &ModalHost,
    inbox_note_id: String,
    inbox_note_title: String,
    inbox_db: Rc<RefCell<Option<InboxDb>>>,
    workspace_db: Rc<RefCell<Option<WorkspaceDb>>>,
    known_ws: Rc<RefCell<KnownWorkspaceRegistry>>,
    on_placed: impl Fn(PlacedInfo) + 'static,
) {
    // ── Collect workspaces for the picker ─────────────────────────────────────
    let workspace_list: Vec<(PathBuf, String)> = {
        let kw = known_ws.borrow();
        kw.list()
            .iter()
            .filter(|w| w.path.exists())
            .map(|w| (w.path.clone(), w.display_name.clone()))
            .collect()
    };

    if workspace_list.is_empty() {
        host.show_error(
            "No workspaces available",
            "Open or create a .water workspace from Desk Mode, then try Place Note again.",
        );
        return;
    }

    // The toplevel window — used to parent the small inline name prompts, which
    // stay as transient windows (the host shows one modal at a time, so a nested
    // host modal would replace this picker).
    let app_window: Option<gtk::Window> = host
        .overlay
        .root()
        .and_then(|r| r.downcast::<gtk::Window>().ok());

    // ── Compute suggestion ────────────────────────────────────────────────────
    let suggestion = {
        let ws_guard = workspace_db.borrow();
        let kw = known_ws.borrow();
        compute_suggestion(ws_guard.as_ref(), &kw)
    };

    // ── Build the dialog content (hosted as an overlay) ───────────────────────
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    vbox.add_css_class("place-note-dialog");
    vbox.set_size_request(460, -1);

    // ── Note title display ────────────────────────────────────────────────────
    let note_title_lbl = gtk::Label::new(Some(&format!("Note: \"{inbox_note_title}\"")));
    note_title_lbl.add_css_class("place-note-title");
    note_title_lbl.set_halign(gtk::Align::Start);
    note_title_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
    note_title_lbl.set_max_width_chars(60);
    vbox.append(&note_title_lbl);

    let sep1 = gtk::Separator::new(gtk::Orientation::Horizontal);
    sep1.set_margin_top(12);
    sep1.set_margin_bottom(12);
    vbox.append(&sep1);

    // ── Workspace row ─────────────────────────────────────────────────────────
    let ws_row = make_label_row("Workspace:");
    let ws_combo = gtk::ComboBoxText::new();
    ws_combo.add_css_class("place-note-combo");
    ws_combo.set_hexpand(true);

    for (_, name) in &workspace_list {
        ws_combo.append_text(name);
    }

    // Pre-select suggested workspace.
    let suggested_ws_idx = suggestion
        .as_ref()
        .and_then(|s| {
            workspace_list
                .iter()
                .position(|(p, _)| *p == s.workspace_path)
        })
        .unwrap_or(0) as u32;
    ws_combo.set_active(Some(suggested_ws_idx));

    ws_row.append(&ws_combo);
    vbox.append(&ws_row);

    // ── Room row ──────────────────────────────────────────────────────────────
    let room_row = make_label_row("Room:");
    let room_combo = gtk::ComboBoxText::new();
    room_combo.add_css_class("place-note-combo");
    room_combo.set_hexpand(true);

    let new_room_btn = gtk::Button::with_label("+ New Room");
    new_room_btn.add_css_class("place-note-new-btn");

    room_row.append(&room_combo);
    room_row.append(&new_room_btn);
    vbox.append(&room_row);

    // ── Destination kind ──────────────────────────────────────────────────────
    let kind_lbl = gtk::Label::new(Some("Place in:"));
    kind_lbl.add_css_class("place-note-field-label");
    kind_lbl.set_halign(gtk::Align::Start);
    kind_lbl.set_margin_top(8);
    vbox.append(&kind_lbl);

    let radio_loose = gtk::CheckButton::with_label("Loose Notes");
    radio_loose.add_css_class("place-note-radio");
    radio_loose.set_margin_start(8);

    let radio_shelf = gtk::CheckButton::with_label("Shelf:");
    radio_shelf.add_css_class("place-note-radio");
    radio_shelf.set_margin_start(8);
    radio_shelf.set_group(Some(&radio_loose));

    let radio_pile = gtk::CheckButton::with_label("Pile:");
    radio_pile.add_css_class("place-note-radio");
    radio_pile.set_margin_start(8);
    radio_pile.set_group(Some(&radio_loose));

    radio_loose.set_active(true);

    vbox.append(&radio_loose);

    // ── Shelf row ─────────────────────────────────────────────────────────────
    let shelf_row = make_indented_combo_row();
    let shelf_combo = gtk::ComboBoxText::new();
    shelf_combo.add_css_class("place-note-combo");
    shelf_combo.set_hexpand(true);
    shelf_combo.set_sensitive(false);

    let new_shelf_btn = gtk::Button::with_label("+ New Shelf");
    new_shelf_btn.add_css_class("place-note-new-btn");
    new_shelf_btn.set_sensitive(false);

    shelf_row.append(&radio_shelf);
    shelf_row.append(&shelf_combo);
    shelf_row.append(&new_shelf_btn);
    vbox.append(&shelf_row);

    // ── Pile row ──────────────────────────────────────────────────────────────
    let pile_row = make_indented_combo_row();
    let pile_combo = gtk::ComboBoxText::new();
    pile_combo.add_css_class("place-note-combo");
    pile_combo.set_hexpand(true);
    pile_combo.set_sensitive(false);

    let new_pile_btn = gtk::Button::with_label("+ New Pile");
    new_pile_btn.add_css_class("place-note-new-btn");
    new_pile_btn.set_sensitive(false);

    pile_row.append(&radio_pile);
    pile_row.append(&pile_combo);
    pile_row.append(&new_pile_btn);
    vbox.append(&pile_row);

    // ── Suggested label ───────────────────────────────────────────────────────
    let suggestion_lbl = gtk::Label::new(None);
    suggestion_lbl.add_css_class("place-note-suggestion");
    suggestion_lbl.set_halign(gtk::Align::Start);
    suggestion_lbl.set_margin_top(10);
    vbox.append(&suggestion_lbl);

    // ── Error label ───────────────────────────────────────────────────────────
    let error_lbl = gtk::Label::new(None);
    error_lbl.add_css_class("place-note-error");
    error_lbl.set_halign(gtk::Align::Start);
    error_lbl.set_wrap(true);
    error_lbl.set_margin_top(8);
    error_lbl.set_visible(false);
    vbox.append(&error_lbl);

    // ── Action buttons (hosted in the modal's actions row) ────────────────────
    let actions = modal_host::build_modal_actions();
    let cancel_btn = modal_host::build_modal_button("Cancel", ButtonKind::Secondary, || {});
    let place_btn = modal_host::build_modal_button("Place Note", ButtonKind::Primary, || {});
    actions.append(&cancel_btn);
    actions.append(&place_btn);

    // ── Shared state ──────────────────────────────────────────────────────────
    let state: Rc<RefCell<DialogState>> = Rc::new(RefCell::new(DialogState {
        workspace_paths: workspace_list.iter().map(|(p, _)| p.clone()).collect(),
        room_ids: Vec::new(),
        container_ids: Vec::new(),
        container_kinds: Vec::new(),
    }));

    // ── Load rooms for the given workspace index ───────────────────────────────
    let load_rooms = {
        let state = state.clone();
        let room_combo = room_combo.clone();
        let shelf_combo = shelf_combo.clone();
        let pile_combo = pile_combo.clone();
        let suggestion = suggestion.clone();
        let workspace_list = workspace_list.clone();

        move |ws_idx: u32| {
            let ws_path = match workspace_list.get(ws_idx as usize) {
                Some((p, _)) => p.clone(),
                None => return,
            };

            let ws = match WorkspaceDb::open(&ws_path) {
                Ok(ws) => ws,
                Err(_) => return,
            };

            let rooms = ws.list_rooms().unwrap_or_default();

            room_combo.remove_all();
            let mut room_ids = Vec::new();

            for room in &rooms {
                room_combo.append_text(&room.name);
                room_ids.push(room.id.clone());
            }

            // Pre-select suggested room if on the suggested workspace.
            let suggested_room_idx = suggestion
                .as_ref()
                .filter(|s| s.workspace_path == ws_path)
                .and_then(|s| s.room_id.as_ref())
                .and_then(|rid| room_ids.iter().position(|id| id == rid))
                .unwrap_or(0) as u32;

            state.borrow_mut().room_ids = room_ids;

            if !rooms.is_empty() {
                room_combo.set_active(Some(suggested_room_idx));
            }

            // Clear container combos — load_containers will repopulate.
            shelf_combo.remove_all();
            pile_combo.remove_all();
            state.borrow_mut().container_ids.clear();
            state.borrow_mut().container_kinds.clear();
        }
    };

    // ── Load containers for the given room ────────────────────────────────────
    let load_containers = {
        let state = state.clone();
        let shelf_combo = shelf_combo.clone();
        let pile_combo = pile_combo.clone();
        let workspace_list = workspace_list.clone();

        move |ws_idx: u32, room_idx: u32| {
            let ws_path = match workspace_list.get(ws_idx as usize) {
                Some((p, _)) => p.clone(),
                None => return,
            };

            let room_id = {
                let s = state.borrow();
                match s.room_ids.get(room_idx as usize).cloned() {
                    Some(id) => id,
                    None => return,
                }
            };

            let ws = match WorkspaceDb::open(&ws_path) {
                Ok(ws) => ws,
                Err(_) => return,
            };

            let containers = ws.list_containers_in_room(&room_id).unwrap_or_default();

            shelf_combo.remove_all();
            pile_combo.remove_all();

            let mut container_ids = Vec::new();
            let mut container_kinds = Vec::new();

            for c in &containers {
                match c.kind {
                    ContainerKind::Shelf => shelf_combo.append_text(&c.name),
                    ContainerKind::Pile => pile_combo.append_text(&c.name),
                }
                container_ids.push(c.id.clone());
                container_kinds.push(c.kind.clone());
            }

            let shelf_count = containers
                .iter()
                .filter(|c| c.kind == ContainerKind::Shelf)
                .count();
            let pile_count = containers
                .iter()
                .filter(|c| c.kind == ContainerKind::Pile)
                .count();

            let mut s = state.borrow_mut();
            s.container_ids = container_ids;
            s.container_kinds = container_kinds;

            if shelf_count > 0 {
                shelf_combo.set_active(Some(0));
            }
            if pile_count > 0 {
                pile_combo.set_active(Some(0));
            }
        }
    };

    // ── Wire workspace change → reload rooms ──────────────────────────────────
    {
        let load_rooms = load_rooms.clone();
        let load_containers = load_containers.clone();
        ws_combo.connect_changed(move |combo| {
            let idx = combo.active().unwrap_or(0);
            load_rooms(idx);
            load_containers(idx, 0);
        });
    }

    // ── Wire room change → reload containers ──────────────────────────────────
    {
        let load_containers = load_containers.clone();
        let ws_combo = ws_combo.clone();
        room_combo.connect_changed(move |combo| {
            let ws_idx = ws_combo.active().unwrap_or(0);
            let room_idx = combo.active().unwrap_or(0);
            load_containers(ws_idx, room_idx);
        });
    }

    // ── Wire destination-kind radio buttons ───────────────────────────────────
    {
        let shelf_combo = shelf_combo.clone();
        let pile_combo = pile_combo.clone();
        let new_shelf_btn = new_shelf_btn.clone();
        let new_pile_btn = new_pile_btn.clone();

        let update_kind_sensitivity = move |loose: bool, shelf: bool, pile: bool| {
            shelf_combo.set_sensitive(shelf);
            pile_combo.set_sensitive(pile);
            new_shelf_btn.set_sensitive(shelf);
            new_pile_btn.set_sensitive(pile);
            let _ = loose; // Loose Notes needs no combo.
        };

        let uks_loose = update_kind_sensitivity.clone();
        radio_loose.connect_toggled(move |btn| {
            if btn.is_active() {
                uks_loose(true, false, false);
            }
        });

        let uks_shelf = update_kind_sensitivity.clone();
        radio_shelf.connect_toggled(move |btn| {
            if btn.is_active() {
                uks_shelf(false, true, false);
            }
        });

        radio_pile.connect_toggled(move |btn| {
            if btn.is_active() {
                update_kind_sensitivity(false, false, true);
            }
        });
    }

    // ── Wire "+ New Room" ─────────────────────────────────────────────────────
    {
        let win_ref = app_window.clone();
        let ws_combo = ws_combo.clone();
        let room_combo = room_combo.clone();
        let state = state.clone();
        let load_containers = load_containers.clone();
        let workspace_list = workspace_list.clone();

        new_room_btn.connect_clicked(move |_| {
            let ws_idx = ws_combo.active().unwrap_or(0);
            let ws_path = match workspace_list.get(ws_idx as usize) {
                Some((p, _)) => p.clone(),
                None => return,
            };
            let ws = match WorkspaceDb::open(&ws_path) {
                Ok(ws) => ws,
                Err(_) => return,
            };

            let Some(win) = win_ref.as_ref() else { return };
            if let Some(name) = prompt_name(win, "New Room", "Room name:") {
                if let Ok(room) = ws.create_room(&name) {
                    // Append to combo and select the new room.
                    room_combo.append_text(&name);
                    let new_idx = state.borrow().room_ids.len() as u32;
                    state.borrow_mut().room_ids.push(room.id.clone());
                    room_combo.set_active(Some(new_idx));
                    load_containers(ws_idx, new_idx);
                }
            }
        });
    }

    // ── Wire "+ New Shelf" ────────────────────────────────────────────────────
    {
        let win_ref = app_window.clone();
        let ws_combo = ws_combo.clone();
        let room_combo = room_combo.clone();
        let state = state.clone();
        let shelf_combo = shelf_combo.clone();
        let workspace_list = workspace_list.clone();

        new_shelf_btn.connect_clicked(move |_| {
            let ws_idx = ws_combo.active().unwrap_or(0);
            let room_idx = room_combo.active().unwrap_or(0);
            let (ws_path, room_id) = {
                let s = state.borrow();
                let p = match workspace_list.get(ws_idx as usize) {
                    Some((p, _)) => p.clone(),
                    None => return,
                };
                let r = match s.room_ids.get(room_idx as usize).cloned() {
                    Some(id) => id,
                    None => return,
                };
                (p, r)
            };
            let ws = match WorkspaceDb::open(&ws_path) {
                Ok(ws) => ws,
                Err(_) => return,
            };
            let Some(win) = win_ref.as_ref() else { return };
            if let Some(name) = prompt_name(win, "New Shelf", "Shelf name:") {
                if let Ok(shelf) = ws.create_container(&room_id, &name, ContainerKind::Shelf) {
                    shelf_combo.append_text(&name);
                    let new_idx = {
                        let mut s = state.borrow_mut();
                        let idx = s.container_ids.len() as u32;
                        s.container_ids.push(shelf.id.clone());
                        s.container_kinds.push(ContainerKind::Shelf);
                        idx
                    };
                    shelf_combo.set_active(Some(new_idx));
                }
            }
        });
    }

    // ── Wire "+ New Pile" ─────────────────────────────────────────────────────
    {
        let win_ref = app_window.clone();
        let ws_combo = ws_combo.clone();
        let room_combo = room_combo.clone();
        let state = state.clone();
        let pile_combo = pile_combo.clone();
        let workspace_list = workspace_list.clone();

        new_pile_btn.connect_clicked(move |_| {
            let ws_idx = ws_combo.active().unwrap_or(0);
            let room_idx = room_combo.active().unwrap_or(0);
            let (ws_path, room_id) = {
                let s = state.borrow();
                let p = match workspace_list.get(ws_idx as usize) {
                    Some((p, _)) => p.clone(),
                    None => return,
                };
                let r = match s.room_ids.get(room_idx as usize).cloned() {
                    Some(id) => id,
                    None => return,
                };
                (p, r)
            };
            let ws = match WorkspaceDb::open(&ws_path) {
                Ok(ws) => ws,
                Err(_) => return,
            };
            let Some(win) = win_ref.as_ref() else { return };
            if let Some(name) = prompt_name(win, "New Pile", "Pile name:") {
                if let Ok(pile) = ws.create_container(&room_id, &name, ContainerKind::Pile) {
                    pile_combo.append_text(&name);
                    let new_idx = {
                        let mut s = state.borrow_mut();
                        let idx = s.container_ids.len() as u32;
                        s.container_ids.push(pile.id.clone());
                        s.container_kinds.push(ContainerKind::Pile);
                        idx
                    };
                    pile_combo.set_active(Some(new_idx));
                }
            }
        });
    }

    // ── Cancel button ─────────────────────────────────────────────────────────
    {
        let host_ref = host.clone();
        cancel_btn.connect_clicked(move |_| {
            host_ref.hide();
        });
    }

    // ── Place Note button ─────────────────────────────────────────────────────
    let on_placed = Rc::new(on_placed);
    {
        let host_ref = host.clone();
        let ws_combo = ws_combo.clone();
        let room_combo = room_combo.clone();
        let shelf_combo = shelf_combo.clone();
        let pile_combo = pile_combo.clone();
        let radio_loose = radio_loose.clone();
        let radio_shelf = radio_shelf.clone();
        let state = state.clone();
        let inbox_db = inbox_db.clone();
        let known_ws = known_ws.clone();
        let error_lbl = error_lbl.clone();
        let inbox_note_id = inbox_note_id.clone();

        place_btn.connect_clicked(move |_| {
            // Resolve destination.
            let ws_idx = ws_combo.active().unwrap_or(0);
            let room_idx = room_combo.active().unwrap_or(0);

            let (ws_path, room_id) = {
                let s = state.borrow();
                let p = match s.selected_workspace_path(ws_idx).cloned() {
                    Some(p) => p,
                    None => {
                        show_error(&error_lbl, "Please select a workspace.");
                        return;
                    }
                };
                let r = match s.selected_room_id(room_idx) {
                    Some(id) => id.to_string(),
                    None => {
                        show_error(&error_lbl, "Please select a room.");
                        return;
                    }
                };
                (p, r)
            };

            let destination = if radio_loose.is_active() {
                PlaceDestination::LooseInRoom {
                    room_id: room_id.clone(),
                }
            } else if radio_shelf.is_active() {
                let idx = shelf_combo.active().unwrap_or(0);
                let s = state.borrow();
                match s.selected_container_id(idx) {
                    Some(id) => PlaceDestination::InContainer {
                        room_id: room_id.clone(),
                        container_id: id.to_string(),
                    },
                    None => {
                        show_error(
                            &error_lbl,
                            "Please select a shelf, or create one with + New Shelf.",
                        );
                        return;
                    }
                }
            } else {
                // Pile radio is active.
                let idx = pile_combo.active().unwrap_or(0);
                let s = state.borrow();
                match s.selected_container_id(idx) {
                    Some(id) => PlaceDestination::InContainer {
                        room_id: room_id.clone(),
                        container_id: id.to_string(),
                    },
                    None => {
                        show_error(
                            &error_lbl,
                            "Please select a pile, or create one with + New Pile.",
                        );
                        return;
                    }
                }
            };

            // Determine container for registry update.
            let (container_id_for_reg, container_kind_for_reg) = match &destination {
                PlaceDestination::LooseInRoom { .. } => (None, None),
                PlaceDestination::InContainer { container_id, .. } => {
                    let idx = if radio_shelf.is_active() {
                        shelf_combo.active().unwrap_or(0)
                    } else {
                        pile_combo.active().unwrap_or(0)
                    };
                    let kind = state
                        .borrow()
                        .selected_container_kind(idx)
                        .map(|k| k.as_str().to_string());
                    (Some(container_id.clone()), kind)
                }
            };

            // Execute placement.
            let request = PlacementRequest {
                inbox_note_id: inbox_note_id.clone(),
                workspace_path: ws_path.clone(),
                destination,
            };

            let result = {
                let guard = inbox_db.borrow();
                match guard.as_ref() {
                    Some(db) => place_inbox_note(db, &request),
                    None => {
                        show_error(&error_lbl, "Inbox database is not available.");
                        return;
                    }
                }
            };

            match result {
                Ok(placed) => {
                    // Update known-workspace registry with last-used destination.
                    record_last_used_destination(
                        &mut known_ws.borrow_mut(),
                        &ws_path,
                        &room_id,
                        container_id_for_reg.as_deref(),
                        container_kind_for_reg.as_deref(),
                    );

                    let info = PlacedInfo {
                        workspace_note_id: placed.workspace_note_id,
                        workspace_path: placed.workspace_path,
                        workspace_name: placed.workspace_name,
                        destination_label: placed.destination_label,
                    };
                    host_ref.hide();
                    (on_placed)(info);
                }
                Err(e) => {
                    show_error(&error_lbl, &e.to_string());
                }
            }
        });
    }

    // Escape is handled by the modal host (scrim_dismisses).

    // ── Initial population ────────────────────────────────────────────────────
    // Trigger room + container load for the initially selected workspace.
    let initial_ws_idx = ws_combo.active().unwrap_or(0);
    load_rooms(initial_ws_idx);
    {
        let room_idx = room_combo.active().unwrap_or(0);
        load_containers(initial_ws_idx, room_idx);
    }

    // Pre-select container based on suggestion.
    if let Some(sug) = &suggestion {
        if let Some(cid) = &sug.container_id {
            let s = state.borrow();
            if let Some(idx) = s.container_ids.iter().position(|id| id == cid) {
                match s.container_kinds.get(idx) {
                    Some(ContainerKind::Shelf) => {
                        let shelf_idx = s.container_ids[..idx]
                            .iter()
                            .zip(s.container_kinds.iter())
                            .filter(|(_, k)| **k == ContainerKind::Shelf)
                            .count() as u32;
                        shelf_combo.set_active(Some(shelf_idx));
                    }
                    Some(ContainerKind::Pile) => {
                        let pile_idx = s.container_ids[..idx]
                            .iter()
                            .zip(s.container_kinds.iter())
                            .filter(|(_, k)| **k == ContainerKind::Pile)
                            .count() as u32;
                        pile_combo.set_active(Some(pile_idx));
                    }
                    None => {}
                }
            }
        }

        suggestion_lbl.set_text(&format!("Suggested: {}", sug.workspace_name));
    }

    host.show_with_custom_ui("Place Note", &vbox, &actions, true, None);
}

// ── PlacedInfo — returned to the caller on success ────────────────────────────

/// Info about a successfully placed note, passed to the `on_placed` callback.
#[derive(Debug, Clone)]
pub struct PlacedInfo {
    pub workspace_note_id: String,
    pub workspace_path: PathBuf,
    pub workspace_name: String,
    pub destination_label: String,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_label_row(label: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.set_margin_top(6);
    let lbl = gtk::Label::new(Some(label));
    lbl.add_css_class("place-note-field-label");
    lbl.set_width_chars(12);
    lbl.set_halign(gtk::Align::Start);
    row.append(&lbl);
    row
}

fn make_indented_combo_row() -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.set_margin_top(4);
    row.set_margin_start(8);
    row
}

fn show_error(label: &gtk::Label, msg: &str) {
    label.set_text(msg);
    label.set_visible(true);
}

/// Show a small modal prompt asking for a name string.
/// Returns `None` if the user cancelled or left the entry blank.
fn prompt_name(parent: &gtk::Window, title: &str, prompt: &str) -> Option<String> {
    let dlg = gtk::Window::builder()
        .transient_for(parent)
        .modal(true)
        .title(title)
        .default_width(320)
        .default_height(120)
        .resizable(false)
        .build();

    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 8);
    vbox.set_margin_top(16);
    vbox.set_margin_bottom(16);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);

    let lbl = gtk::Label::new(Some(prompt));
    lbl.set_halign(gtk::Align::Start);
    vbox.append(&lbl);

    let entry = gtk::Entry::new();
    entry.set_activates_default(true);
    vbox.append(&entry);

    let btn_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    btn_row.set_halign(gtk::Align::End);

    let cancel = gtk::Button::with_label("Cancel");
    let ok_btn = gtk::Button::with_label("Create");
    ok_btn.add_css_class("suggested-action");
    btn_row.append(&cancel);
    btn_row.append(&ok_btn);
    vbox.append(&btn_row);

    dlg.set_child(Some(&vbox));

    // Use a shared result cell.
    let result: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));

    {
        let dlg_ref = dlg.clone();
        cancel.connect_clicked(move |_| dlg_ref.close());
    }
    {
        let dlg_ref = dlg.clone();
        let entry_ref = entry.clone();
        let result_ref = result.clone();
        ok_btn.connect_clicked(move |_| {
            let text = entry_ref.text().to_string();
            if !text.trim().is_empty() {
                *result_ref.borrow_mut() = Some(text.trim().to_string());
            }
            dlg_ref.close();
        });
    }

    // Run the dialog synchronously by spinning the GTK main loop until it closes.
    let done = Rc::new(std::cell::Cell::new(false));
    {
        let done = done.clone();
        dlg.connect_close_request(move |_| {
            done.set(true);
            gtk::glib::Propagation::Proceed
        });
    }

    dlg.present();
    entry.grab_focus();

    // Pump the event loop until the dialog is dismissed.
    let ctx = glib::MainContext::default();
    while !done.get() {
        ctx.iteration(true);
    }

    let r = result.borrow().clone();
    r
}
