use super::modal_host::{self, ButtonKind, ModalHost};
use gtk::prelude::*;

/// All commands available in the command palette.
/// Commands that are implemented log stderr; real dispatch happens via callbacks.
pub const COMMANDS: &[&str] = &[
    // ── Desk ──────────────────────────────────────────────────────────────
    "Open Desk",
    "Close Desk",
    "Open Focused Workspace",
    "Switch Workspace",
    "Pin Current Note",
    "Unpin Current Note",
    // ── Navigation ────────────────────────────────────────────────────────
    "Search",
    "Search All Workspaces",
    "Open Room Map",
    // ── Room Map ──────────────────────────────────────────────────────────
    "Create Room",
    "Connect Rooms",
    "Change Room Connection Type",
    "Remove Room Connection",
    "Open Selected Room",
    // ── Tabs and windows ───────────────────────────────────────────────────
    "New Inbox Note",
    "New Workspace Note",
    "Close Tab",
    "Next Tab",
    "Previous Tab",
    "Open Current Note in New Window",
    "New Window",
    // ── Note creation ─────────────────────────────────────────────────────
    "Place Note",
    // ── Workspace organization ─────────────────────────────────────────────
    "Create Shelf",
    "Create Pile",
    "Convert Pile to Shelf",
    // ── Note operations ────────────────────────────────────────────────────
    "Attach Palette",
    "Split Note",
    "Merge Notes",
    "Bookmark Version",
    "Show Version History",
    "Toggle Markdown Source",
    "Attach Image",
    "Open Linked File",
    "Absorb File",
    // ── View modes ─────────────────────────────────────────────────────────
    "Open Compare Mode",
    "Open Arrange Mode",
    // ── Export ─────────────────────────────────────────────────────────────
    "Export Note",
    "Export All Notes",
];

/// Open the command palette as an in-window modal overlay on `host`.
/// `status_label` is updated with the last selected command name.
/// `on_place_note` is called when the user activates "Place Note".
/// `on_room_map_cmd` is called with the command name for Room Map commands.
/// `on_general_cmd` is called for all other wired commands (tabs, windows,
/// compare mode, etc.).
pub fn open(
    host: &ModalHost,
    status_label: &gtk::Label,
    on_place_note: Option<std::rc::Rc<dyn Fn()>>,
    on_room_map_cmd: Option<std::rc::Rc<dyn Fn(&str)>>,
    on_general_cmd: Option<std::rc::Rc<dyn Fn(&str)>>,
) {
    // Content layout (search + list); the host supplies the panel chrome.
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    vbox.add_css_class("command-palette-window");

    // Search entry
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Type a command…"));
    search.add_css_class("command-palette-search");
    search.set_margin_bottom(10);

    let sep = gtk::Separator::new(gtk::Orientation::Horizontal);

    // Scrollable command list — fixed size since the panel sizes to content.
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(360)
        .min_content_width(500)
        .build();
    scrolled.set_margin_top(8);

    let list = gtk::ListBox::new();
    list.add_css_class("command-palette-list");
    list.set_selection_mode(gtk::SelectionMode::Browse);

    for &cmd in COMMANDS {
        let row = gtk::ListBoxRow::new();
        row.add_css_class("command-palette-row");
        let label = gtk::Label::builder()
            .label(cmd)
            .halign(gtk::Align::Start)
            .margin_start(16)
            .margin_end(16)
            .margin_top(7)
            .margin_bottom(7)
            .build();
        row.set_child(Some(&label));
        list.append(&row);
    }

    scrolled.set_child(Some(&list));

    vbox.append(&search);
    vbox.append(&sep);
    vbox.append(&scrolled);

    // --- Filter as you type ---
    let list_clone = list.clone();
    search.connect_search_changed(move |entry| {
        let text = entry.text().to_lowercase();
        let mut idx: i32 = 0;
        loop {
            let Some(row) = list_clone.row_at_index(idx) else {
                break;
            };
            let visible = text.is_empty()
                || row
                    .child()
                    .and_then(|w| w.downcast::<gtk::Label>().ok())
                    .map(|lbl| lbl.label().to_lowercase().contains(&text))
                    .unwrap_or(false);
            row.set_visible(visible);
            idx += 1;
        }
        // Select the first visible row after filtering.
        let mut sel_idx: i32 = 0;
        loop {
            let Some(row) = list_clone.row_at_index(sel_idx) else {
                break;
            };
            if row.is_visible() {
                list_clone.select_row(Some(&row));
                break;
            }
            sel_idx += 1;
        }
    });

    // --- Activate on row click or Enter ---
    let host_for_list = host.clone();
    let status_for_list = status_label.clone();
    let place_for_list = on_place_note.clone();
    let room_map_for_list = on_room_map_cmd.clone();
    let general_for_list = on_general_cmd.clone();
    list.connect_row_activated(move |_, row| {
        activate_command(
            row,
            &status_for_list,
            place_for_list.as_deref(),
            room_map_for_list.as_deref(),
            general_for_list.as_deref(),
        );
        host_for_list.hide();
    });

    // --- Keyboard handling inside the search entry ---
    // Enter activates the selected row; Escape is handled by the modal host.
    let key_ctrl = gtk::EventControllerKey::new();
    let list_for_key = list.clone();
    let host_for_key = host.clone();
    let status_for_key = status_label.clone();
    let place_for_key = on_place_note.clone();
    let room_map_for_key = on_room_map_cmd.clone();
    let general_for_key = on_general_cmd.clone();
    key_ctrl.connect_key_pressed(move |_, key, _, _| match key {
        gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter => {
            if let Some(row) = list_for_key.selected_row() {
                activate_command(
                    &row,
                    &status_for_key,
                    place_for_key.as_deref(),
                    room_map_for_key.as_deref(),
                    general_for_key.as_deref(),
                );
                host_for_key.hide();
            }
            glib::Propagation::Stop
        }
        _ => glib::Propagation::Proceed,
    });
    search.add_controller(key_ctrl);

    // --- Select the first row by default ---
    if let Some(first) = list.row_at_index(0) {
        list.select_row(Some(&first));
    }

    // Single Close action fills the host's actions row.
    let actions = modal_host::build_modal_actions();
    let host_for_close = host.clone();
    let close_btn =
        modal_host::build_modal_button("Close", ButtonKind::Secondary, move || {
            host_for_close.hide()
        });
    actions.append(&close_btn);

    host.show_with_custom_ui("Commands", &vbox, &actions, true, None);
    search.grab_focus();
}

const ROOM_MAP_COMMANDS: &[&str] = &[
    "Create Room",
    "Connect Rooms",
    "Change Room Connection Type",
    "Remove Room Connection",
    "Open Selected Room",
    "Open Room Map",
];

const GENERAL_COMMANDS: &[&str] = &[
    "New Inbox Note",
    "New Workspace Note",
    "Close Tab",
    "Next Tab",
    "Previous Tab",
    "Open Current Note in New Window",
    "New Window",
    "Open Compare Mode",
    "Split Note",
    "Merge Notes",
    "Bookmark Version",
    "Show Version History",
    "Absorb File",
];

fn activate_command(
    row: &gtk::ListBoxRow,
    status_label: &gtk::Label,
    on_place_note: Option<&dyn Fn()>,
    on_room_map_cmd: Option<&dyn Fn(&str)>,
    on_general_cmd: Option<&dyn Fn(&str)>,
) {
    let Some(label) = row.child().and_then(|w| w.downcast::<gtk::Label>().ok()) else {
        return;
    };
    let cmd = label.label().to_string();
    eprintln!("blot: command palette → {cmd}");
    status_label.set_text(&format!("Command: {cmd}"));

    if cmd == "Place Note" {
        if let Some(f) = on_place_note {
            f();
        }
        return;
    }

    if ROOM_MAP_COMMANDS.contains(&cmd.as_str()) {
        if let Some(f) = on_room_map_cmd {
            f(&cmd);
        } else {
            eprintln!("blot: room map command '{cmd}' — open Room Map first");
        }
        return;
    }

    if GENERAL_COMMANDS.contains(&cmd.as_str()) {
        if let Some(f) = on_general_cmd {
            f(&cmd);
        } else {
            eprintln!("blot: command '{cmd}' — handler not available in this context");
        }
    }
}
