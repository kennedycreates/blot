pub mod absorb_dialog;
pub mod command_palette;
pub mod modal_host;
pub mod compare_shell;
pub mod desk_shell;
pub mod editor_shell;
pub mod external_file_shell;
pub mod main_window;
pub mod merge_dialog;
pub mod place_note_dialog;
pub mod room_map_shell;
pub mod search_shell;
pub mod tab_bar;
pub mod version_history_shell;
pub mod water_workspace_shell;
pub mod workspace_shell;

use gtk::prelude::*;

/// Set the status-bar save label's text and drive its visual feedback class.
///
/// `"Saved"` gets a one-shot brass flash that fades to the resting dim color;
/// transient pending states (`"Unsaved"`, `"Saving…"`) get a steady amber;
/// everything else (e.g. `"New note"`, errors) stays neutral. Removing then
/// re-adding the class re-triggers the CSS flash on every save.
pub fn set_save_status(label: &gtk::Label, text: &str) {
    label.remove_css_class("status-saved");
    label.remove_css_class("status-unsaved");
    label.set_text(text);
    if text.starts_with("Saved") {
        // "Saved", "Saved to file" — a successful write. ("Save error" excluded.)
        label.add_css_class("status-saved");
    } else if text.contains("Unsaved") || text == "Saving…" {
        label.add_css_class("status-unsaved");
    }
}
