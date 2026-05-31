//! Tab model and tab-strip widget for Blot's multi-note working model.
//!
//! `TabModel` is pure Rust (no GTK) and fully testable.
//! `TabBar` wraps a GTK widget and delegates user gestures to closures.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

// ── NoteSource ────────────────────────────────────────────────────────────────

/// Where a note lives — used to route tab loads and autosaves correctly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteSource {
    Inbox,
    Workspace(PathBuf),
}

impl NoteSource {
    /// Short label shown in the tab subtitle / tooltip.
    pub fn location_label(&self, workspace_name: &str) -> String {
        match self {
            NoteSource::Inbox => "Inbox".to_string(),
            NoteSource::Workspace(_) => workspace_name.to_string(),
        }
    }

    pub fn is_inbox(&self) -> bool {
        matches!(self, NoteSource::Inbox)
    }

    pub fn workspace_path(&self) -> Option<&PathBuf> {
        match self {
            NoteSource::Workspace(p) => Some(p),
            NoteSource::Inbox => None,
        }
    }
}

// ── TabEntry ──────────────────────────────────────────────────────────────────

/// One open note tab.
#[derive(Debug, Clone)]
pub struct TabEntry {
    pub tab_id: String,
    /// `None` for a brand-new unsaved blank note.
    pub note_id: Option<String>,
    pub source: NoteSource,
    /// Title shown in the tab button.  Updated on every successful save.
    pub title: String,
}

fn new_tab_id() -> String {
    static CTR: AtomicU64 = AtomicU64::new(1);
    format!("tab{:016x}", CTR.fetch_add(1, Ordering::Relaxed))
}

// ── TabModel ──────────────────────────────────────────────────────────────────

/// Pure-Rust tab list.  No GTK dependency — fully unit-testable.
#[derive(Debug, Default)]
pub struct TabModel {
    tabs: Vec<TabEntry>,
    active_idx: usize,
}

impl TabModel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a note in the tab strip.
    ///
    /// If a tab for `note_id` already exists, focus it and return `(tab_id, false)`.
    /// Otherwise create a new tab at the end and return `(tab_id, true)`.
    pub fn open_or_focus(
        &mut self,
        note_id: &str,
        source: NoteSource,
        title: &str,
    ) -> (String, bool) {
        if let Some(idx) = self
            .tabs
            .iter()
            .position(|t| t.note_id.as_deref() == Some(note_id))
        {
            let tab_id = self.tabs[idx].tab_id.clone();
            self.active_idx = idx;
            return (tab_id, false);
        }
        let entry = TabEntry {
            tab_id: new_tab_id(),
            note_id: Some(note_id.to_string()),
            source,
            title: title.to_string(),
        };
        let tab_id = entry.tab_id.clone();
        self.tabs.push(entry);
        self.active_idx = self.tabs.len() - 1;
        (tab_id, true)
    }

    /// Add a new blank (unsaved) tab. Returns the new tab ID.
    pub fn new_blank(&mut self, source: NoteSource) -> String {
        let entry = TabEntry {
            tab_id: new_tab_id(),
            note_id: None,
            source,
            title: "New note".to_string(),
        };
        let tab_id = entry.tab_id.clone();
        self.tabs.push(entry);
        self.active_idx = self.tabs.len() - 1;
        tab_id
    }

    /// Close the tab with the given `tab_id`.
    /// Returns `true` if found and removed.
    /// Adjusts `active_idx` so it stays in bounds.
    pub fn close(&mut self, tab_id: &str) -> bool {
        let Some(idx) = self.tabs.iter().position(|t| t.tab_id == tab_id) else {
            return false;
        };
        self.tabs.remove(idx);
        if self.tabs.is_empty() {
            self.active_idx = 0;
        } else if self.active_idx >= self.tabs.len() {
            self.active_idx = self.tabs.len() - 1;
        }
        true
    }

    /// Make the tab with `tab_id` active.  Returns `true` on success.
    pub fn set_active_by_id(&mut self, tab_id: &str) -> bool {
        if let Some(idx) = self.tabs.iter().position(|t| t.tab_id == tab_id) {
            self.active_idx = idx;
            true
        } else {
            false
        }
    }

    /// Advance to the next tab (wraps around).
    pub fn next_tab(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        self.active_idx = (self.active_idx + 1) % self.tabs.len();
    }

    /// Go back to the previous tab (wraps around).
    pub fn prev_tab(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        if self.active_idx == 0 {
            self.active_idx = self.tabs.len() - 1;
        } else {
            self.active_idx -= 1;
        }
    }

    pub fn active_tab(&self) -> Option<&TabEntry> {
        self.tabs.get(self.active_idx)
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut TabEntry> {
        self.tabs.get_mut(self.active_idx)
    }

    pub fn tabs(&self) -> &[TabEntry] {
        &self.tabs
    }

    pub fn active_idx(&self) -> usize {
        self.active_idx
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Update the display title of the active tab (called after autosave).
    pub fn update_active_title(&mut self, title: &str) {
        if let Some(t) = self.active_tab_mut() {
            t.title = title.to_string();
        }
    }

    /// Assign a saved note_id to the active tab (called when a blank tab is
    /// first saved and gets a real ID).
    pub fn update_active_note_id(&mut self, note_id: &str) {
        if let Some(t) = self.active_tab_mut() {
            if t.note_id.is_none() {
                t.note_id = Some(note_id.to_string());
            }
        }
    }

    /// Return the tab index for the given note_id, if any tab is showing it.
    pub fn find_tab_for_note(&self, note_id: &str) -> Option<usize> {
        self.tabs
            .iter()
            .position(|t| t.note_id.as_deref() == Some(note_id))
    }

    /// Return the `NoteSource` of the tab with the given `tab_id`, if it exists.
    pub fn find_source_by_tab_id(&self, tab_id: &str) -> Option<NoteSource> {
        self.tabs
            .iter()
            .find(|t| t.tab_id == tab_id)
            .map(|t| t.source.clone())
    }
}

// ── TabBar (GTK widget) ───────────────────────────────────────────────────────

use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// Truncate a string to at most `max_chars` characters, appending "…" if cut.
fn truncate_title(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = chars[..max_chars].iter().collect();
        format!("{truncated}…")
    }
}

/// GTK tab strip.  Cheap to clone — all inner state is `Rc`-wrapped.
#[derive(Clone)]
pub struct TabBar {
    pub root: gtk::Box,
    tabs_box: gtk::Box,
    model: Rc<RefCell<TabModel>>,
    on_switch: Rc<dyn Fn(String)>,
    on_close: Rc<dyn Fn(String)>,
    on_new: Rc<dyn Fn()>,
}

impl TabBar {
    pub fn new(
        model: Rc<RefCell<TabModel>>,
        on_switch: impl Fn(String) + 'static,
        on_close: impl Fn(String) + 'static,
        on_new: impl Fn() + 'static,
    ) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("tab-bar");

        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .hexpand(true)
            .build();
        scroll.add_css_class("tab-bar-scroll");

        let tabs_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        tabs_box.add_css_class("tab-strip");
        scroll.set_child(Some(&tabs_box));
        root.append(&scroll);

        TabBar {
            root,
            tabs_box,
            model,
            on_switch: Rc::new(on_switch),
            on_close: Rc::new(on_close),
            on_new: Rc::new(on_new),
        }
    }

    /// Rebuild the visual tab strip from the current model state.
    /// Call this after any model mutation.
    pub fn refresh(&self) {
        // Clear existing children.
        while let Some(child) = self.tabs_box.first_child() {
            self.tabs_box.remove(&child);
        }

        let m = self.model.borrow();

        for (i, tab) in m.tabs().iter().enumerate() {
            let is_active = i == m.active_idx();
            let tab_id = tab.tab_id.clone();

            let item = gtk::Box::new(gtk::Orientation::Horizontal, 2);
            item.add_css_class("tab-item");
            if is_active {
                item.add_css_class("tab-item-active");
            }

            // Source badge (small "I" or "W" label).
            let badge = gtk::Label::new(Some(if tab.source.is_inbox() { "I" } else { "W" }));
            badge.add_css_class("tab-badge");
            badge.set_tooltip_text(Some(if tab.source.is_inbox() {
                "Inbox note"
            } else {
                "Workspace note"
            }));

            let title_btn = gtk::Button::new();
            title_btn.add_css_class("tab-title-btn");
            let label = gtk::Label::new(Some(&truncate_title(&tab.title, 22)));
            label.add_css_class("tab-title-label");
            label.set_tooltip_text(Some(&tab.title));
            title_btn.set_child(Some(&label));

            let close_btn = gtk::Button::with_label("×");
            close_btn.add_css_class("tab-close-btn");
            close_btn.set_tooltip_text(Some("Close tab (saves first)"));

            item.append(&badge);
            item.append(&title_btn);
            item.append(&close_btn);
            self.tabs_box.append(&item);

            // Wire clicks.
            let on_switch = self.on_switch.clone();
            let id1 = tab_id.clone();
            title_btn.connect_clicked(move |_| on_switch(id1.clone()));

            let on_close = self.on_close.clone();
            let id2 = tab_id.clone();
            close_btn.connect_clicked(move |_| on_close(id2.clone()));
        }

        // "+" new tab button.
        let new_btn = gtk::Button::with_label("+");
        new_btn.add_css_class("tab-new-btn");
        new_btn.set_tooltip_text(Some("New Inbox note tab (Ctrl+N)"));
        let on_new = self.on_new.clone();
        new_btn.connect_clicked(move |_| on_new());
        self.tabs_box.append(&new_btn);

        drop(m);

        // Show the bar only if there are tabs.
        self.root.set_visible(!self.model.borrow().is_empty());
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn inbox() -> NoteSource {
        NoteSource::Inbox
    }

    fn ws(path: &str) -> NoteSource {
        NoteSource::Workspace(PathBuf::from(path))
    }

    #[test]
    fn empty_model_has_no_active_tab() {
        let m = TabModel::new();
        assert!(m.active_tab().is_none());
        assert!(m.is_empty());
    }

    #[test]
    fn open_or_focus_creates_new_tab() {
        let mut m = TabModel::new();
        let (id, created) = m.open_or_focus("note1", inbox(), "Note One");
        assert!(created);
        assert!(!id.is_empty());
        assert_eq!(m.len(), 1);
        assert_eq!(m.active_tab().unwrap().title, "Note One");
    }

    #[test]
    fn open_or_focus_reuses_existing_tab() {
        let mut m = TabModel::new();
        let (id1, _) = m.open_or_focus("note1", inbox(), "Note One");
        let (id2, created) = m.open_or_focus("note1", inbox(), "Note One");
        assert!(!created);
        assert_eq!(id1, id2);
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn open_two_different_notes() {
        let mut m = TabModel::new();
        m.open_or_focus("a", inbox(), "A");
        m.open_or_focus("b", inbox(), "B");
        assert_eq!(m.len(), 2);
        assert_eq!(m.active_tab().unwrap().note_id.as_deref(), Some("b"));
    }

    #[test]
    fn new_blank_tab_has_no_note_id() {
        let mut m = TabModel::new();
        m.new_blank(inbox());
        let tab = m.active_tab().unwrap();
        assert!(tab.note_id.is_none());
        assert_eq!(tab.title, "New note");
    }

    #[test]
    fn update_active_note_id_assigns_first_id() {
        let mut m = TabModel::new();
        m.new_blank(inbox());
        m.update_active_note_id("new_id");
        assert_eq!(m.active_tab().unwrap().note_id.as_deref(), Some("new_id"));
    }

    #[test]
    fn update_active_note_id_does_not_overwrite_existing() {
        let mut m = TabModel::new();
        m.open_or_focus("existing", inbox(), "Title");
        m.update_active_note_id("other");
        // Should NOT overwrite — note_id was already Some
        assert_eq!(m.active_tab().unwrap().note_id.as_deref(), Some("existing"));
    }

    #[test]
    fn close_removes_tab_and_adjusts_active() {
        let mut m = TabModel::new();
        m.open_or_focus("a", inbox(), "A");
        let (b_id, _) = m.open_or_focus("b", inbox(), "B");
        assert_eq!(m.active_idx(), 1);
        m.close(&b_id);
        assert_eq!(m.len(), 1);
        assert_eq!(m.active_idx(), 0);
    }

    #[test]
    fn close_first_tab_focuses_remaining() {
        let mut m = TabModel::new();
        let (a_id, _) = m.open_or_focus("a", inbox(), "A");
        m.open_or_focus("b", inbox(), "B");
        m.set_active_by_id(&a_id);
        m.close(&a_id);
        assert_eq!(m.len(), 1);
        assert_eq!(m.active_idx(), 0);
        assert_eq!(m.active_tab().unwrap().note_id.as_deref(), Some("b"));
    }

    #[test]
    fn close_unknown_tab_returns_false() {
        let mut m = TabModel::new();
        assert!(!m.close("doesnotexist"));
    }

    #[test]
    fn close_last_tab_leaves_empty_model() {
        let mut m = TabModel::new();
        let (id, _) = m.open_or_focus("a", inbox(), "A");
        m.close(&id);
        assert!(m.is_empty());
        assert_eq!(m.active_idx(), 0);
    }

    #[test]
    fn next_tab_wraps_around() {
        let mut m = TabModel::new();
        m.open_or_focus("a", inbox(), "A");
        m.open_or_focus("b", inbox(), "B");
        m.open_or_focus("c", inbox(), "C");
        m.set_active_by_id(m.tabs()[2].tab_id.clone().as_str());
        m.next_tab();
        assert_eq!(m.active_idx(), 0);
    }

    #[test]
    fn prev_tab_wraps_around() {
        let mut m = TabModel::new();
        m.open_or_focus("a", inbox(), "A");
        m.open_or_focus("b", inbox(), "B");
        m.set_active_by_id(m.tabs()[0].tab_id.clone().as_str());
        m.prev_tab();
        assert_eq!(m.active_idx(), 1);
    }

    #[test]
    fn update_active_title_changes_display_title() {
        let mut m = TabModel::new();
        m.open_or_focus("n", inbox(), "Old");
        m.update_active_title("New Title");
        assert_eq!(m.active_tab().unwrap().title, "New Title");
    }

    #[test]
    fn find_tab_for_note_returns_correct_index() {
        let mut m = TabModel::new();
        m.open_or_focus("a", inbox(), "A");
        m.open_or_focus("b", inbox(), "B");
        assert_eq!(m.find_tab_for_note("a"), Some(0));
        assert_eq!(m.find_tab_for_note("b"), Some(1));
        assert_eq!(m.find_tab_for_note("c"), None);
    }

    #[test]
    fn workspace_source_returns_path() {
        let src = ws("/home/user/notes.water");
        assert!(!src.is_inbox());
        assert_eq!(
            src.workspace_path(),
            Some(&PathBuf::from("/home/user/notes.water"))
        );
    }

    #[test]
    fn inbox_source_has_no_path() {
        let src = inbox();
        assert!(src.is_inbox());
        assert!(src.workspace_path().is_none());
    }
}
