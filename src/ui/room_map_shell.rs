//! Room Map Mode — visual and list view of Rooms and Doors in a workspace.
//!
//! The shell has two sub-views toggled by the toolbar:
//! - Map view: Cairo-rendered canvas with draggable room cards and connection lines.
//! - List view: selected-room detail panel showing shelves, piles, and connections.
//!
//! Works only with SQLite-backed workspaces (`WorkspaceDb`). JSON water files
//! do not have `blot_rooms` / `blot_room_connections` tables and show an info
//! state instead.

use super::modal_host::{self, ButtonKind, ModalHost};
use crate::workspace::{Room, RoomConnection, WorkspaceDb};
use cairo::Context as CairoCtx;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

// ── Canvas constants ──────────────────────────────────────────────────────────

const CARD_W: f64 = 164.0;
const CARD_H: f64 = 88.0;
const CARD_R: f64 = 8.0; // corner radius
const CANVAS_MIN_W: i32 = 600;
const CANVAS_MIN_H: i32 = 460;

// ── Colour palette (dark watercolor theme) ────────────────────────────────────

// Room card fill
const CARD_BG: (f64, f64, f64) = (0.157, 0.149, 0.133); // near-charcoal
const CARD_BG_SEL: (f64, f64, f64) = (0.204, 0.192, 0.165); // slightly lighter when selected
                                                            // Room card border
const CARD_BORDER: (f64, f64, f64, f64) = (0.47, 0.43, 0.35, 0.6); // muted brass
const CARD_BORDER_SEL: (f64, f64, f64, f64) = (0.78, 0.69, 0.39, 1.0); // bright brass
                                                                       // Text
const TEXT_NAME: (f64, f64, f64) = (0.90, 0.88, 0.82); // cream
const TEXT_STATS: (f64, f64, f64, f64) = (0.63, 0.59, 0.51, 0.85); // muted vellum
                                                                   // Connections
const CONN_NORMAL: (f64, f64, f64, f64) = (0.59, 0.55, 0.45, 0.55);
const CONN_STRONG: (f64, f64, f64, f64) = (0.78, 0.69, 0.39, 0.85); // brass
const CONN_WEAK: (f64, f64, f64, f64) = (0.47, 0.43, 0.35, 0.35);
// Canvas background
const CANVAS_BG: (f64, f64, f64) = (0.102, 0.098, 0.086);

// ── MapState ──────────────────────────────────────────────────────────────────

/// In-memory snapshot of the room map — rebuilt each time `refresh()` is called.
#[derive(Default)]
struct MapState {
    rooms: Vec<Room>,
    /// Parallel to `rooms`: (note_count, container_count).
    stats: Vec<(i64, i64)>,
    connections: Vec<RoomConnection>,
    selected_room_id: Option<String>,
    /// If dragging: (room_id, press_x_in_card, press_y_in_card).
    drag: Option<(String, f64, f64)>,
    /// True if positions were auto-laid out (not from DB) this session.
    auto_laid_out: bool,
}

impl MapState {
    fn room_index(&self, room_id: &str) -> Option<usize> {
        self.rooms.iter().position(|r| r.id == room_id)
    }

    /// Return the center point of a room card on the canvas.
    fn room_center(&self, room_id: &str) -> Option<(f64, f64)> {
        self.rooms
            .iter()
            .find(|r| r.id == room_id)
            .map(|r| (r.map_x + CARD_W / 2.0, r.map_y + CARD_H / 2.0))
    }

    /// Find which room (if any) contains the canvas point (x, y).
    fn room_at(&self, x: f64, y: f64) -> Option<&str> {
        self.rooms
            .iter()
            .find(|r| {
                x >= r.map_x && x <= r.map_x + CARD_W && y >= r.map_y && y <= r.map_y + CARD_H
            })
            .map(|r| r.id.as_str())
    }

    /// Compute an automatic circular layout for all rooms placed at (0, 0).
    /// Only runs when every room is at the default (0, 0) position.
    fn auto_layout(&mut self) {
        let n = self.rooms.len();
        if n == 0 {
            return;
        }
        let all_default = self.rooms.iter().all(|r| r.map_x == 0.0 && r.map_y == 0.0);
        if !all_default {
            return;
        }
        self.auto_laid_out = true;
        if n == 1 {
            let r = &mut self.rooms[0];
            r.map_x = (CANVAS_MIN_W as f64 / 2.0) - CARD_W / 2.0;
            r.map_y = (CANVAS_MIN_H as f64 / 2.0) - CARD_H / 2.0;
            return;
        }
        let cx = CANVAS_MIN_W as f64 / 2.0;
        let cy = CANVAS_MIN_H as f64 / 2.0;
        let radius = (n as f64 * 85.0).max(180.0).min(210.0);
        use std::f64::consts::PI;
        for (i, room) in self.rooms.iter_mut().enumerate() {
            let angle = (i as f64 * 2.0 * PI / n as f64) - PI / 2.0;
            room.map_x = cx + radius * angle.cos() - CARD_W / 2.0;
            room.map_y = cy + radius * angle.sin() - CARD_H / 2.0;
        }
    }
}

// ── RoomMapShell ──────────────────────────────────────────────────────────────

/// Room Map Mode surface. Clone is cheap — all internals are Rc-wrapped.
#[derive(Clone)]
pub struct RoomMapShell {
    /// Root widget added to the mode stack.
    pub root: gtk::Box,
    workspace_db: Rc<RefCell<Option<WorkspaceDb>>>,
    map_state: Rc<RefCell<MapState>>,
    canvas: gtk::DrawingArea,
    sidebar_list: gtk::ListBox,
    ws_label: gtk::Label,
    detail_box: gtk::Box,
    detail_scroll: gtk::ScrolledWindow,
    main_stack: gtk::Stack,
    is_map_view: Rc<Cell<bool>>,
    map_btn: gtk::ToggleButton,
    list_btn: gtk::ToggleButton,
    modal_host: ModalHost,
    on_open_room: Rc<dyn Fn(String)>,
}

impl RoomMapShell {
    pub fn new(
        workspace_db: Rc<RefCell<Option<WorkspaceDb>>>,
        modal_host: ModalHost,
        on_open_room: impl Fn(String) + 'static,
    ) -> Self {
        let map_state: Rc<RefCell<MapState>> = Rc::new(RefCell::new(MapState::default()));
        let is_map_view: Rc<Cell<bool>> = Rc::new(Cell::new(true));
        let on_open_room: Rc<dyn Fn(String)> = Rc::new(on_open_room);

        // ── Root ─────────────────────────────────────────────────────────────
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("room-map-shell");

        // ── Toolbar ───────────────────────────────────────────────────────────
        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        toolbar.add_css_class("room-map-toolbar");
        toolbar.set_margin_start(12);
        toolbar.set_margin_end(12);
        toolbar.set_margin_top(8);
        toolbar.set_margin_bottom(8);

        // View toggle: Map / List
        let view_toggle_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        view_toggle_box.add_css_class("linked");

        let map_btn = gtk::ToggleButton::with_label("Map");
        map_btn.add_css_class("mode-button");
        map_btn.set_active(true);
        map_btn.set_tooltip_text(Some("Show visual Room Map canvas"));

        let list_btn = gtk::ToggleButton::with_label("List");
        list_btn.add_css_class("mode-button");
        list_btn.set_tooltip_text(Some("Show Room detail list"));
        list_btn.set_group(Some(&map_btn));

        view_toggle_box.append(&map_btn);
        view_toggle_box.append(&list_btn);

        let sep1 = gtk::Separator::new(gtk::Orientation::Vertical);
        sep1.set_margin_start(4);
        sep1.set_margin_end(4);

        let add_room_btn = gtk::Button::with_label("+ Room");
        add_room_btn.add_css_class("mode-button");
        add_room_btn.set_tooltip_text(Some("Create a new Room"));

        let connect_btn = gtk::Button::with_label("+ Connect");
        connect_btn.add_css_class("mode-button");
        connect_btn.set_tooltip_text(Some("Add a Door between two Rooms"));

        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);

        let ws_label = gtk::Label::new(Some("No workspace"));
        ws_label.add_css_class("room-map-ws-label");
        ws_label.set_halign(gtk::Align::End);
        ws_label.set_ellipsize(gtk::pango::EllipsizeMode::End);

        toolbar.append(&view_toggle_box);
        toolbar.append(&sep1);
        toolbar.append(&add_room_btn);
        toolbar.append(&connect_btn);
        toolbar.append(&spacer);
        toolbar.append(&ws_label);

        root.append(&toolbar);
        root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

        // ── Content area (sidebar + main) ────────────────────────────────────
        let content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        content.set_vexpand(true);
        content.set_hexpand(true);

        // ── Sidebar ───────────────────────────────────────────────────────────
        let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 0);
        sidebar.add_css_class("room-map-sidebar");
        sidebar.set_size_request(240, -1);

        let sidebar_header = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        sidebar_header.add_css_class("room-map-sidebar-header");
        sidebar_header.set_margin_start(12);
        sidebar_header.set_margin_end(8);
        sidebar_header.set_margin_top(10);
        sidebar_header.set_margin_bottom(6);

        let rooms_lbl = gtk::Label::new(Some("Rooms"));
        rooms_lbl.add_css_class("room-map-section-label");
        rooms_lbl.set_halign(gtk::Align::Start);
        rooms_lbl.set_hexpand(true);

        sidebar_header.append(&rooms_lbl);
        sidebar.append(&sidebar_header);

        let sidebar_scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let sidebar_list = gtk::ListBox::new();
        sidebar_list.add_css_class("room-map-room-list");
        sidebar_list.set_selection_mode(gtk::SelectionMode::Single);
        sidebar_scroll.set_child(Some(&sidebar_list));
        sidebar.append(&sidebar_scroll);

        content.append(&sidebar);
        content.append(&gtk::Separator::new(gtk::Orientation::Vertical));

        // ── Main area (stack: map canvas | list detail) ───────────────────────
        let main_stack = gtk::Stack::new();
        main_stack.set_hexpand(true);
        main_stack.set_vexpand(true);
        main_stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        main_stack.set_transition_duration(80);

        // Map canvas page
        let canvas_scroll = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .vexpand(true)
            .build();
        canvas_scroll.add_css_class("room-map-canvas-scroll");

        let canvas = gtk::DrawingArea::new();
        canvas.set_size_request(CANVAS_MIN_W, CANVAS_MIN_H);
        canvas.set_hexpand(true);
        canvas.set_vexpand(true);
        canvas_scroll.set_child(Some(&canvas));
        main_stack.add_named(&canvas_scroll, Some("map"));

        // Detail panel page
        let detail_scroll = gtk::ScrolledWindow::builder()
            .hexpand(true)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let detail_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        detail_box.set_margin_top(24);
        detail_box.set_margin_start(24);
        detail_box.set_margin_end(24);
        detail_scroll.set_child(Some(&detail_box));
        main_stack.add_named(&detail_scroll, Some("detail"));

        content.append(&main_stack);
        root.append(&content);

        // ── Build the shell struct ────────────────────────────────────────────
        let shell = RoomMapShell {
            root,
            workspace_db,
            map_state,
            canvas,
            sidebar_list: sidebar_list.clone(),
            ws_label,
            detail_box: detail_box.clone(),
            detail_scroll,
            main_stack: main_stack.clone(),
            is_map_view: is_map_view.clone(),
            map_btn: map_btn.clone(),
            list_btn: list_btn.clone(),
            modal_host,
            on_open_room,
        };

        // ── Wire view toggle buttons ──────────────────────────────────────────
        {
            let shell2 = shell.clone();
            map_btn.connect_toggled(move |btn| {
                if btn.is_active() {
                    shell2.is_map_view.set(true);
                    shell2.main_stack.set_visible_child_name("map");
                    shell2.canvas.queue_draw();
                }
            });
        }
        {
            let shell2 = shell.clone();
            list_btn.connect_toggled(move |btn| {
                if btn.is_active() {
                    shell2.is_map_view.set(false);
                    shell2.main_stack.set_visible_child_name("detail");
                    shell2.rebuild_detail_panel();
                }
            });
        }

        // ── Sidebar: select room ──────────────────────────────────────────────
        {
            let shell2 = shell.clone();
            sidebar_list.connect_row_activated(move |_, row| {
                let room_id = row.widget_name().to_string();
                if !room_id.is_empty() {
                    shell2.select_room(&room_id);
                }
            });
        }

        // ── Add Room button ───────────────────────────────────────────────────
        {
            let shell2 = shell.clone();
            add_room_btn.connect_clicked(move |_| {
                shell2.prompt_create_room();
            });
        }

        // ── Connect button ────────────────────────────────────────────────────
        {
            let shell2 = shell.clone();
            connect_btn.connect_clicked(move |_| {
                shell2.show_connect_dialog();
            });
        }

        // ── Canvas: draw function ─────────────────────────────────────────────
        {
            let state = shell.map_state.clone();
            shell.canvas.set_draw_func(move |_area, cr, width, height| {
                draw_canvas(cr, width, height, &state.borrow());
            });
        }

        // ── Canvas: click to select / double-click to navigate ────────────────
        {
            let shell2 = shell.clone();
            let click = gtk::GestureClick::new();
            click.set_button(1);
            let shell3 = shell2.clone();
            click.connect_released(move |gesture, n_press, x, y| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                let room_id = {
                    let state = shell3.map_state.borrow();
                    state.room_at(x, y).map(|s| s.to_string())
                };
                if let Some(id) = room_id {
                    if n_press == 2 {
                        // Double-click → navigate to room
                        (shell3.on_open_room)(id);
                    } else {
                        // Single click → select
                        shell3.select_room(&id);
                    }
                }
            });
            shell.canvas.add_controller(click);
        }

        // ── Canvas: drag to reposition room ──────────────────────────────────
        {
            let shell2 = shell.clone();
            let drag = gtk::GestureDrag::new();

            let shell_begin = shell2.clone();
            drag.connect_drag_begin(move |_, x, y| {
                let room_id = {
                    let state = shell_begin.map_state.borrow();
                    state.room_at(x, y).map(|s| s.to_string())
                };
                if let Some(id) = room_id {
                    let (off_x, off_y) = {
                        let state = shell_begin.map_state.borrow();
                        if let Some(idx) = state.room_index(&id) {
                            (x - state.rooms[idx].map_x, y - state.rooms[idx].map_y)
                        } else {
                            (0.0, 0.0)
                        }
                    };
                    shell_begin.map_state.borrow_mut().drag = Some((id, off_x, off_y));
                }
            });

            let shell_update = shell2.clone();
            drag.connect_drag_update(move |gesture, dx, dy| {
                let start = gesture.start_point();
                if let Some((sx, sy)) = start {
                    let abs_x = sx + dx;
                    let abs_y = sy + dy;
                    let mut state = shell_update.map_state.borrow_mut();
                    if let Some((ref id, off_x, off_y)) = state.drag.clone() {
                        let new_x = (abs_x - off_x).max(0.0);
                        let new_y = (abs_y - off_y).max(0.0);
                        if let Some(idx) = state.room_index(id) {
                            state.rooms[idx].map_x = new_x;
                            state.rooms[idx].map_y = new_y;
                        }
                    }
                    drop(state);
                    shell_update.canvas.queue_draw();
                }
            });

            let shell_end = shell2.clone();
            drag.connect_drag_end(move |_, _, _| {
                let drag_room = shell_end.map_state.borrow().drag.clone();
                if let Some((id, _, _)) = drag_room {
                    let (x, y) = {
                        let state = shell_end.map_state.borrow();
                        state
                            .room_index(&id)
                            .map(|i| (state.rooms[i].map_x, state.rooms[i].map_y))
                            .unwrap_or((0.0, 0.0))
                    };
                    let db_guard = shell_end.workspace_db.borrow();
                    if let Some(db) = db_guard.as_ref() {
                        let _ = db.update_room_map_position(&id, x, y);
                    }
                    shell_end.map_state.borrow_mut().drag = None;
                }
            });

            shell.canvas.add_controller(drag);
        }

        shell
    }

    // ── Public API ────────────────────────────────────────────────────────────

    /// Refresh state from the current workspace. Call when the mode becomes visible.
    pub fn refresh(&self) {
        let (rooms, stats, connections, ws_name) = {
            let db_guard = self.workspace_db.borrow();
            if let Some(db) = db_guard.as_ref() {
                let rooms = db.list_rooms().unwrap_or_default();
                let stats = rooms
                    .iter()
                    .map(|r| {
                        (
                            db.room_total_note_count(&r.id),
                            db.room_container_count(&r.id),
                        )
                    })
                    .collect();
                let connections = db.list_room_connections().unwrap_or_default();
                let name = db.workspace_name();
                (rooms, stats, connections, name)
            } else {
                (vec![], vec![], vec![], "No workspace".to_string())
            }
        };

        self.ws_label.set_text(&ws_name);

        {
            let mut state = self.map_state.borrow_mut();
            // Preserve positions for rooms that already have non-zero positions
            // by merging new room list with existing positions.
            let old_positions: std::collections::HashMap<String, (f64, f64)> = state
                .rooms
                .iter()
                .filter(|r| r.map_x != 0.0 || r.map_y != 0.0)
                .map(|r| (r.id.clone(), (r.map_x, r.map_y)))
                .collect();

            state.rooms = rooms;
            state.stats = stats;
            state.connections = connections;
            state.auto_laid_out = false;

            // Restore previously-computed positions (from drag or prior auto-layout).
            for room in state.rooms.iter_mut() {
                if let Some(&(x, y)) = old_positions.get(&room.id) {
                    if room.map_x == 0.0 && room.map_y == 0.0 {
                        room.map_x = x;
                        room.map_y = y;
                    }
                }
            }

            state.auto_layout();
        }

        self.rebuild_sidebar();

        if self.is_map_view.get() {
            self.canvas.queue_draw();
        } else {
            self.rebuild_detail_panel();
        }
    }

    // ── Selection ─────────────────────────────────────────────────────────────

    fn select_room(&self, room_id: &str) {
        self.map_state.borrow_mut().selected_room_id = Some(room_id.to_string());
        self.select_sidebar_row(room_id);
        if !self.is_map_view.get() {
            self.rebuild_detail_panel();
        }
        self.canvas.queue_draw();
    }

    fn select_sidebar_row(&self, room_id: &str) {
        let mut child = self.sidebar_list.first_child();
        while let Some(widget) = child {
            let next = widget.next_sibling();
            if let Ok(row) = widget.clone().downcast::<gtk::ListBoxRow>() {
                if row.widget_name().as_str() == room_id {
                    self.sidebar_list.select_row(Some(&row));
                    break;
                }
            }
            child = next;
        }
    }

    // ── Sidebar rebuild ───────────────────────────────────────────────────────

    fn rebuild_sidebar(&self) {
        while let Some(child) = self.sidebar_list.first_child() {
            self.sidebar_list.remove(&child);
        }

        let state = self.map_state.borrow();
        if state.rooms.is_empty() {
            let hint = gtk::Label::new(Some(if self.workspace_db.borrow().is_none() {
                "Open a workspace to view rooms."
            } else {
                "No rooms yet. Click + Room to add one."
            }));
            hint.add_css_class("room-map-empty-hint");
            hint.set_margin_start(14);
            hint.set_margin_top(12);
            hint.set_halign(gtk::Align::Start);
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&hint));
            row.set_activatable(false);
            self.sidebar_list.append(&row);
            return;
        }

        for (i, room) in state.rooms.iter().enumerate() {
            let (note_count, container_count) = state.stats.get(i).copied().unwrap_or((0, 0));
            let conn_count = state
                .connections
                .iter()
                .filter(|c| c.room_a_id == room.id || c.room_b_id == room.id)
                .count();

            let row = gtk::ListBoxRow::new();
            row.add_css_class("room-map-room-row");
            row.set_widget_name(&room.id);

            let row_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
            row_box.set_margin_start(12);
            row_box.set_margin_end(8);
            row_box.set_margin_top(7);
            row_box.set_margin_bottom(7);

            let name_lbl = gtk::Label::new(Some(&room.name));
            name_lbl.add_css_class("room-map-room-name");
            name_lbl.set_halign(gtk::Align::Start);
            name_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
            row_box.append(&name_lbl);

            let stats_lbl = gtk::Label::new(Some(&format!(
                "{note_count} notes · {container_count} containers · {conn_count} doors"
            )));
            stats_lbl.add_css_class("room-map-room-stats");
            stats_lbl.set_halign(gtk::Align::Start);
            row_box.append(&stats_lbl);

            row.set_child(Some(&row_box));
            self.sidebar_list.append(&row);
        }

        // Re-select the previously selected room.
        if let Some(ref sel) = state.selected_room_id.clone() {
            self.select_sidebar_row(sel);
        }
    }

    // ── Detail panel rebuild ──────────────────────────────────────────────────

    fn rebuild_detail_panel(&self) {
        while let Some(child) = self.detail_box.first_child() {
            self.detail_box.remove(&child);
        }

        let selected = self.map_state.borrow().selected_room_id.clone();

        let Some(room_id) = selected else {
            let hint = gtk::Label::new(Some("Select a Room from the list to see its details."));
            hint.add_css_class("room-map-empty-hint");
            hint.set_halign(gtk::Align::Start);
            self.detail_box.append(&hint);
            return;
        };

        // Collect data while holding DB borrow, then release before building UI.
        struct RoomDetail {
            name: String,
            note_count: i64,
            container_count: i64,
            connections: Vec<(RoomConnection, String)>, // (conn, other_room_name)
        }

        let detail_opt: Option<RoomDetail> = {
            let db_guard = self.workspace_db.borrow();
            db_guard.as_ref().and_then(|db| {
                let room = db.get_room(&room_id).ok()??;
                let note_count = db.room_total_note_count(&room_id);
                let container_count = db.room_container_count(&room_id);
                let conns = db.list_connections_for_room(&room_id).unwrap_or_default();
                let connections = conns
                    .into_iter()
                    .map(|c| {
                        let other_id = if c.room_a_id == room_id {
                            c.room_b_id.clone()
                        } else {
                            c.room_a_id.clone()
                        };
                        let other_name = db
                            .get_room(&other_id)
                            .ok()
                            .flatten()
                            .map(|r| r.name)
                            .unwrap_or_else(|| other_id.clone());
                        (c, other_name)
                    })
                    .collect();
                Some(RoomDetail {
                    name: room.name,
                    note_count,
                    container_count,
                    connections,
                })
            })
        };

        let Some(detail) = detail_opt else {
            let hint = gtk::Label::new(Some("Room not found."));
            hint.add_css_class("room-map-empty-hint");
            self.detail_box.append(&hint);
            return;
        };

        // Room name + Open button
        let header_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        header_box.set_margin_bottom(16);

        let name_lbl = gtk::Label::new(Some(&detail.name));
        name_lbl.add_css_class("room-map-detail-name");
        name_lbl.set_halign(gtk::Align::Start);
        name_lbl.set_hexpand(true);

        let open_btn = gtk::Button::with_label("Open Room");
        open_btn.add_css_class("mode-button");
        open_btn.set_tooltip_text(Some("Switch to Workspace Mode and navigate to this room"));
        {
            let on_open = self.on_open_room.clone();
            let rid = room_id.clone();
            open_btn.connect_clicked(move |_| {
                (on_open)(rid.clone());
            });
        }

        header_box.append(&name_lbl);
        header_box.append(&open_btn);
        self.detail_box.append(&header_box);

        // Stats
        let stats_lbl = gtk::Label::new(Some(&format!(
            "{} notes · {} containers",
            detail.note_count, detail.container_count
        )));
        stats_lbl.add_css_class("room-map-detail-stats");
        stats_lbl.set_halign(gtk::Align::Start);
        stats_lbl.set_margin_bottom(20);
        self.detail_box.append(&stats_lbl);

        // Connections section
        let conn_header_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        conn_header_box.set_margin_bottom(8);

        let conn_section_lbl = gtk::Label::new(Some("Doors (Connections)"));
        conn_section_lbl.add_css_class("room-map-section-label");
        conn_section_lbl.set_halign(gtk::Align::Start);
        conn_section_lbl.set_hexpand(true);

        let add_conn_btn = gtk::Button::with_label("+ Door");
        add_conn_btn.add_css_class("room-map-mini-btn");
        add_conn_btn.set_tooltip_text(Some("Add a Door from this Room to another"));
        {
            let shell2 = self.clone();
            add_conn_btn.connect_clicked(move |_| {
                shell2.show_connect_dialog();
            });
        }

        conn_header_box.append(&conn_section_lbl);
        conn_header_box.append(&add_conn_btn);
        self.detail_box.append(&conn_header_box);

        if detail.connections.is_empty() {
            let no_conns = gtk::Label::new(Some("No doors yet."));
            no_conns.add_css_class("room-map-empty-hint");
            no_conns.set_halign(gtk::Align::Start);
            no_conns.set_margin_bottom(8);
            self.detail_box.append(&no_conns);
        } else {
            for (conn, other_name) in detail.connections {
                self.append_connection_row(&conn, &other_name);
            }
        }
    }

    fn append_connection_row(&self, conn: &RoomConnection, other_name: &str) {
        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row_box.add_css_class("room-map-conn-row");
        row_box.set_margin_bottom(6);

        let type_chip = gtk::Label::new(Some(connection_type_label(&conn.connection_type)));
        type_chip.add_css_class("room-map-conn-type");
        type_chip.add_css_class(&format!("conn-type-{}", conn.connection_type));

        let name_lbl = gtk::Label::new(Some(&format!("↔ {other_name}")));
        name_lbl.add_css_class("room-map-conn-name");
        name_lbl.set_halign(gtk::Align::Start);
        name_lbl.set_hexpand(true);
        name_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let change_btn = gtk::Button::with_label("Change");
        change_btn.add_css_class("room-map-mini-btn");
        change_btn.set_tooltip_text(Some("Change connection type"));
        {
            let shell2 = self.clone();
            let conn_id = conn.id.clone();
            let curr_type = conn.connection_type.clone();
            change_btn.connect_clicked(move |_| {
                shell2.show_change_type_dialog(&conn_id, &curr_type);
            });
        }

        let remove_btn = gtk::Button::with_label("Remove");
        remove_btn.add_css_class("room-map-mini-btn");
        remove_btn.set_tooltip_text(Some("Remove this Door"));
        {
            let shell2 = self.clone();
            let conn_id = conn.id.clone();
            remove_btn.connect_clicked(move |_| {
                let db_guard = shell2.workspace_db.borrow();
                if let Some(db) = db_guard.as_ref() {
                    let _ = db.delete_room_connection(&conn_id);
                }
                drop(db_guard);
                shell2.refresh();
            });
        }

        row_box.append(&type_chip);
        row_box.append(&name_lbl);
        row_box.append(&change_btn);
        row_box.append(&remove_btn);
        self.detail_box.append(&row_box);
    }

    // ── Dialogs ───────────────────────────────────────────────────────────────

    fn prompt_create_room(&self) {
        let shell = self.clone();
        self.modal_host
            .show_input("Create Room", "Room name:", "", "Create", move |name| {
                let name = name.trim().to_string();
                if name.is_empty() {
                    return;
                }
                let db_guard = shell.workspace_db.borrow();
                if let Some(db) = db_guard.as_ref() {
                    if let Err(e) = db.create_room(&name) {
                        eprintln!("blot: create room failed: {e}");
                    }
                }
                drop(db_guard);
                shell.refresh();
            });
    }

    fn show_connect_dialog(&self) {
        let rooms: Vec<(String, String)> = {
            let state = self.map_state.borrow();
            state
                .rooms
                .iter()
                .map(|r| (r.id.clone(), r.name.clone()))
                .collect()
        };

        if rooms.len() < 2 {
            self.modal_host.show_error(
                "Need at least 2 rooms",
                "Create more Rooms before adding a Door between them.",
            );
            return;
        }

        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 12);
        vbox.add_css_class("blot-dialog");
        vbox.set_size_request(340, -1);

        // Pre-select from currently-selected room if any.
        let presel_index: Option<usize> = {
            let sel = self.map_state.borrow().selected_room_id.clone();
            sel.and_then(|id| rooms.iter().position(|(rid, _)| rid == &id))
        };

        // Room A dropdown
        let row_a = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let lbl_a = gtk::Label::new(Some("From:"));
        lbl_a.set_size_request(50, -1);
        let combo_a =
            gtk::DropDown::from_strings(&rooms.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>());
        if let Some(idx) = presel_index {
            combo_a.set_selected(idx as u32);
        }
        row_a.append(&lbl_a);
        row_a.append(&combo_a);

        // Room B dropdown
        let row_b = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let lbl_b = gtk::Label::new(Some("To:"));
        lbl_b.set_size_request(50, -1);
        let combo_b =
            gtk::DropDown::from_strings(&rooms.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>());
        // Default: second room
        combo_b.set_selected(if presel_index == Some(0) { 1 } else { 0 });
        row_b.append(&lbl_b);
        row_b.append(&combo_b);

        // Connection type
        let type_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let type_lbl = gtk::Label::new(Some("Type:"));
        type_lbl.set_size_request(50, -1);
        let type_dropdown = gtk::DropDown::from_strings(&["normal", "strong", "weak"]);
        type_box.append(&type_lbl);
        type_box.append(&type_dropdown);

        let error_lbl = gtk::Label::new(None);
        error_lbl.add_css_class("room-map-error-label");
        error_lbl.set_halign(gtk::Align::Start);
        error_lbl.set_visible(false);

        vbox.append(&row_a);
        vbox.append(&row_b);
        vbox.append(&type_box);
        vbox.append(&error_lbl);

        let actions = modal_host::build_modal_actions();
        let cancel_btn = modal_host::build_modal_button("Cancel", ButtonKind::Secondary, {
            let host = self.modal_host.clone();
            move || host.hide()
        });
        let add_btn = modal_host::build_modal_button("Add Door", ButtonKind::Primary, || {});
        actions.append(&cancel_btn);
        actions.append(&add_btn);

        let shell2 = self.clone();
        let rooms2 = rooms.clone();
        let combo_a2 = combo_a.clone();
        let combo_b2 = combo_b.clone();
        let type_dd2 = type_dropdown.clone();
        let error_lbl2 = error_lbl.clone();
        add_btn.connect_clicked(move |_| {
            let idx_a = combo_a2.selected() as usize;
            let idx_b = combo_b2.selected() as usize;
            let conn_type = match type_dd2.selected() {
                1 => "strong",
                2 => "weak",
                _ => "normal",
            };

            if idx_a == idx_b {
                error_lbl2.set_text("A Room cannot connect to itself.");
                error_lbl2.set_visible(true);
                return;
            }
            if idx_a >= rooms2.len() || idx_b >= rooms2.len() {
                return;
            }
            let room_a = &rooms2[idx_a].0;
            let room_b = &rooms2[idx_b].0;
            let db_guard = shell2.workspace_db.borrow();
            if let Some(db) = db_guard.as_ref() {
                match db.create_room_connection(room_a, room_b, conn_type) {
                    Ok(_) => {}
                    Err(e) => {
                        error_lbl2.set_text(&format!("Error: {e}"));
                        error_lbl2.set_visible(true);
                        return;
                    }
                }
            }
            drop(db_guard);
            shell2.refresh();
            shell2.modal_host.hide();
        });

        self.modal_host
            .show_with_custom_ui("Add Door (Connect Rooms)", &vbox, &actions, true, None);
    }

    fn show_change_type_dialog(&self, conn_id: &str, current_type: &str) {
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 12);
        vbox.add_css_class("blot-dialog");
        vbox.set_size_request(280, -1);

        let lbl = gtk::Label::new(Some("Connection type:"));
        lbl.set_halign(gtk::Align::Start);

        let type_dropdown = gtk::DropDown::from_strings(&["normal", "strong", "weak"]);
        let presel = match current_type {
            "strong" => 1,
            "weak" => 2,
            _ => 0,
        };
        type_dropdown.set_selected(presel);

        vbox.append(&lbl);
        vbox.append(&type_dropdown);

        let actions = modal_host::build_modal_actions();
        let cancel_btn = modal_host::build_modal_button("Cancel", ButtonKind::Secondary, {
            let host = self.modal_host.clone();
            move || host.hide()
        });
        let shell2 = self.clone();
        let conn_id = conn_id.to_string();
        let save_btn = modal_host::build_modal_button("Save", ButtonKind::Primary, move || {
            let new_type = match type_dropdown.selected() {
                1 => "strong",
                2 => "weak",
                _ => "normal",
            };
            let db_guard = shell2.workspace_db.borrow();
            if let Some(db) = db_guard.as_ref() {
                let _ = db.update_room_connection_type(&conn_id, new_type);
            }
            drop(db_guard);
            shell2.refresh();
            shell2.modal_host.hide();
        });
        actions.append(&cancel_btn);
        actions.append(&save_btn);

        self.modal_host
            .show_with_custom_ui("Change Connection Type", &vbox, &actions, true, None);
    }

    /// Show the connect dialog pre-selected to the currently selected room.
    pub fn show_connect_rooms_dialog(&self) {
        self.show_connect_dialog();
    }
}

// ── Canvas drawing ────────────────────────────────────────────────────────────

fn draw_canvas(cr: &CairoCtx, width: i32, height: i32, state: &MapState) {
    let _ = (width, height);

    // Background
    let (r, g, b) = CANVAS_BG;
    cr.set_source_rgb(r, g, b);
    let _ = cr.paint();

    // Draw connections first (behind cards)
    for conn in &state.connections {
        if let (Some(ca), Some(cb)) = (
            state.room_center(&conn.room_a_id),
            state.room_center(&conn.room_b_id),
        ) {
            draw_connection(cr, ca, cb, &conn.connection_type);
        }
    }

    // Draw room cards
    for (i, room) in state.rooms.iter().enumerate() {
        let (note_count, container_count) = state.stats.get(i).copied().unwrap_or((0, 0));
        let selected = state
            .selected_room_id
            .as_deref()
            .map(|s| s == room.id)
            .unwrap_or(false);
        draw_room_card(cr, room, note_count, container_count, selected);
    }
}

fn draw_connection(cr: &CairoCtx, from: (f64, f64), to: (f64, f64), conn_type: &str) {
    let (r, g, b, a) = match conn_type {
        "strong" => CONN_STRONG,
        "weak" => CONN_WEAK,
        _ => CONN_NORMAL,
    };
    let line_width = match conn_type {
        "strong" => 3.0_f64,
        "weak" => 1.0_f64,
        _ => 1.8_f64,
    };

    cr.set_source_rgba(r, g, b, a);
    cr.set_line_width(line_width);

    if conn_type == "weak" {
        cr.set_dash(&[6.0, 4.0], 0.0);
    } else {
        cr.set_dash(&[], 0.0);
    }

    cr.move_to(from.0, from.1);
    cr.line_to(to.0, to.1);
    let _ = cr.stroke();

    // Small dot at midpoint for connection type indicator
    let mx = (from.0 + to.0) / 2.0;
    let my = (from.1 + to.1) / 2.0;
    cr.arc(mx, my, 3.0, 0.0, 2.0 * std::f64::consts::PI);
    let _ = cr.fill();

    cr.set_dash(&[], 0.0); // reset
}

fn draw_room_card(
    cr: &CairoCtx,
    room: &Room,
    note_count: i64,
    container_count: i64,
    selected: bool,
) {
    let x = room.map_x;
    let y = room.map_y;

    // Card fill
    let (r, g, b) = if selected { CARD_BG_SEL } else { CARD_BG };
    cr.set_source_rgb(r, g, b);
    rounded_rect(cr, x, y, CARD_W, CARD_H, CARD_R);
    let _ = cr.fill();

    // Card border
    let (r, g, b, a) = if selected {
        CARD_BORDER_SEL
    } else {
        CARD_BORDER
    };
    cr.set_source_rgba(r, g, b, a);
    cr.set_line_width(if selected { 2.0 } else { 1.0 });
    rounded_rect(cr, x, y, CARD_W, CARD_H, CARD_R);
    let _ = cr.stroke();

    // Room name
    let (r, g, b) = TEXT_NAME;
    cr.set_source_rgb(r, g, b);
    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    cr.set_font_size(12.5);

    // Clip name to card width
    let name = if room.name.len() > 18 {
        format!("{}…", &room.name[..17])
    } else {
        room.name.clone()
    };
    cr.move_to(x + 12.0, y + 30.0);
    let _ = cr.show_text(&name);

    // Stats line
    let (r, g, b, a) = TEXT_STATS;
    cr.set_source_rgba(r, g, b, a);
    cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    cr.set_font_size(10.5);
    cr.move_to(x + 12.0, y + 52.0);
    let _ = cr.show_text(&format!("{note_count} notes"));
    cr.move_to(x + 12.0, y + 68.0);
    let _ = cr.show_text(&format!("{container_count} containers"));
}

fn rounded_rect(cr: &CairoCtx, x: f64, y: f64, w: f64, h: f64, r: f64) {
    use std::f64::consts::PI;
    cr.new_sub_path();
    cr.arc(x + r, y + r, r, PI, 3.0 * PI / 2.0);
    cr.arc(x + w - r, y + r, r, 3.0 * PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.close_path();
}

fn connection_type_label(t: &str) -> &'static str {
    match t {
        "strong" => "strong",
        "weak" => "weak",
        _ => "normal",
    }
}
