//! Search Mode — the central discovery surface in Blot.
//!
//! Full-text search across the Global Inbox and known `.water` workspaces.
//! Scope selector, quick filter chips, rich result cards with context snippets,
//! location breadcrumbs, and pin/checklist/image/link indicators.

use crate::inbox::InboxDb;
use crate::known_workspaces::KnownWorkspaceRegistry;
use crate::search::{
    default_scope, run_search, SearchFilters, SearchQuery, SearchResult, SearchScope,
};
use crate::workspace::WorkspaceDb;
use gtk::prelude::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

// ── Public callback target ─────────────────────────────────────────────────────

/// Which note the user wants to open from a search result.
#[derive(Debug, Clone)]
pub enum SearchNoteTarget {
    InboxNote {
        note_id: String,
    },
    WorkspaceNote {
        note_id: String,
        workspace_path: PathBuf,
    },
}

// ── SearchShell ────────────────────────────────────────────────────────────────

/// The Search Mode surface. Clone is cheap — all inner types are Rc-wrapped.
#[derive(Clone)]
pub struct SearchShell {
    pub root: gtk::Box,

    // ── Core search widgets
    search_entry: gtk::Entry,
    results_list: gtk::ListBox,
    empty_label: gtk::Label,
    status_label: gtk::Label,
    unavailable_label: gtk::Label,

    // ── Scope toggle buttons (stored so we can update active state)
    scope_inbox_btn: gtk::ToggleButton,
    scope_ws_btn: gtk::ToggleButton,
    scope_both_btn: gtk::ToggleButton,
    scope_all_btn: gtk::ToggleButton,

    // ── Filter chips
    filter_pinned: gtk::ToggleButton,
    filter_checklist: gtk::ToggleButton,
    filter_image: gtk::ToggleButton,
    filter_links: gtk::ToggleButton,

    // ── Shared state
    inbox_db: Rc<RefCell<Option<InboxDb>>>,
    workspace_db: Rc<RefCell<Option<WorkspaceDb>>>,
    known_ws: Rc<RefCell<KnownWorkspaceRegistry>>,

    current_scope: Rc<RefCell<SearchScope>>,

    on_open_note: Rc<dyn Fn(SearchNoteTarget)>,
    on_place_inbox_note: Rc<dyn Fn(String, String)>,
}

impl SearchShell {
    pub fn new(
        inbox_db: Rc<RefCell<Option<InboxDb>>>,
        workspace_db: Rc<RefCell<Option<WorkspaceDb>>>,
        known_ws: Rc<RefCell<KnownWorkspaceRegistry>>,
        on_open_note: impl Fn(SearchNoteTarget) + 'static,
        on_place_inbox_note: impl Fn(String, String) + 'static,
    ) -> Self {
        // ── Initial scope ──────────────────────────────────────────────────
        let has_ws = workspace_db.borrow().is_some();
        let initial_scope = default_scope(has_ws);
        let current_scope = Rc::new(RefCell::new(initial_scope.clone()));

        // ── Root container ─────────────────────────────────────────────────
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("search-shell");

        // ── Top bar: search entry + clear button ───────────────────────────
        let top_bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        top_bar.add_css_class("search-top-bar");
        top_bar.set_margin_top(12);
        top_bar.set_margin_start(16);
        top_bar.set_margin_end(16);
        top_bar.set_margin_bottom(8);

        let search_entry = gtk::Entry::new();
        search_entry.set_placeholder_text(Some("Search notes…"));
        search_entry.add_css_class("search-entry");
        search_entry.set_hexpand(true);

        let clear_btn = gtk::Button::with_label("×");
        clear_btn.add_css_class("search-clear-btn");
        clear_btn.set_tooltip_text(Some("Clear search"));

        top_bar.append(&search_entry);
        top_bar.append(&clear_btn);

        // ── Scope selector ─────────────────────────────────────────────────
        let scope_bar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        scope_bar.add_css_class("search-scope-bar");
        scope_bar.set_margin_start(16);
        scope_bar.set_margin_end(16);
        scope_bar.set_margin_bottom(6);

        let scope_label = gtk::Label::new(Some("Scope:"));
        scope_label.add_css_class("search-scope-label");

        let scope_inbox_btn = gtk::ToggleButton::with_label("Inbox");
        scope_inbox_btn.add_css_class("scope-chip");
        let scope_ws_btn = gtk::ToggleButton::with_label("Workspace");
        scope_ws_btn.add_css_class("scope-chip");
        let scope_both_btn = gtk::ToggleButton::with_label("Workspace + Inbox");
        scope_both_btn.add_css_class("scope-chip");
        let scope_all_btn = gtk::ToggleButton::with_label("All Workspaces");
        scope_all_btn.add_css_class("scope-chip");

        // Radio-button logic: each button deactivates others.
        // Done via signal connections below after we have all refs.
        scope_bar.append(&scope_label);
        scope_bar.append(&scope_inbox_btn);
        scope_bar.append(&scope_ws_btn);
        scope_bar.append(&scope_both_btn);
        scope_bar.append(&scope_all_btn);

        // ── Filter chips ───────────────────────────────────────────────────
        let filter_bar = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        filter_bar.add_css_class("search-filter-bar");
        filter_bar.set_margin_start(16);
        filter_bar.set_margin_end(16);
        filter_bar.set_margin_bottom(8);

        let filter_label = gtk::Label::new(Some("Filter:"));
        filter_label.add_css_class("search-filter-label");

        let filter_pinned = gtk::ToggleButton::with_label("★ Pinned");
        filter_pinned.add_css_class("filter-chip");
        let filter_checklist = gtk::ToggleButton::with_label("✓ Checklist");
        filter_checklist.add_css_class("filter-chip");
        let filter_image = gtk::ToggleButton::with_label("🖼 Image");
        filter_image.add_css_class("filter-chip");
        let filter_links = gtk::ToggleButton::with_label("🔗 Links");
        filter_links.add_css_class("filter-chip");

        filter_bar.append(&filter_label);
        filter_bar.append(&filter_pinned);
        filter_bar.append(&filter_checklist);
        filter_bar.append(&filter_image);
        filter_bar.append(&filter_links);

        // ── Separator ─────────────────────────────────────────────────────
        let sep = gtk::Separator::new(gtk::Orientation::Horizontal);

        // ── Status line (result count / errors) ───────────────────────────
        let status_label = gtk::Label::new(None);
        status_label.add_css_class("search-status");
        status_label.set_halign(gtk::Align::Start);
        status_label.set_margin_start(16);
        status_label.set_margin_top(4);
        status_label.set_margin_bottom(2);

        let unavailable_label = gtk::Label::new(None);
        unavailable_label.add_css_class("search-unavailable");
        unavailable_label.set_halign(gtk::Align::Start);
        unavailable_label.set_margin_start(16);
        unavailable_label.set_margin_bottom(4);
        unavailable_label.set_visible(false);

        // ── Results list ───────────────────────────────────────────────────
        let results_list = gtk::ListBox::new();
        results_list.add_css_class("search-results-list");
        results_list.set_selection_mode(gtk::SelectionMode::Single);

        let scrolled = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        scrolled.set_child(Some(&results_list));

        // ── Empty state ────────────────────────────────────────────────────
        let empty_label = gtk::Label::new(Some(
            "Type to search. Results show title, snippet, location, and date.",
        ));
        empty_label.add_css_class("search-empty");
        empty_label.set_halign(gtk::Align::Center);
        empty_label.set_valign(gtk::Align::Center);
        empty_label.set_vexpand(true);
        empty_label.set_wrap(true);
        empty_label.set_justify(gtk::Justification::Center);
        empty_label.set_margin_start(32);
        empty_label.set_margin_end(32);

        // Stack: results list vs. empty state
        let content_stack = gtk::Stack::new();
        content_stack.set_vexpand(true);
        content_stack.add_named(&scrolled, Some("results"));
        content_stack.add_named(&empty_label, Some("empty"));
        content_stack.set_visible_child_name("empty");

        root.append(&top_bar);
        root.append(&scope_bar);
        root.append(&filter_bar);
        root.append(&sep);
        root.append(&status_label);
        root.append(&unavailable_label);
        root.append(&content_stack);

        let shell = SearchShell {
            root,
            search_entry: search_entry.clone(),
            results_list: results_list.clone(),
            empty_label: empty_label.clone(),
            status_label: status_label.clone(),
            unavailable_label: unavailable_label.clone(),
            scope_inbox_btn: scope_inbox_btn.clone(),
            scope_ws_btn: scope_ws_btn.clone(),
            scope_both_btn: scope_both_btn.clone(),
            scope_all_btn: scope_all_btn.clone(),
            filter_pinned: filter_pinned.clone(),
            filter_checklist: filter_checklist.clone(),
            filter_image: filter_image.clone(),
            filter_links: filter_links.clone(),
            inbox_db,
            workspace_db,
            known_ws,
            current_scope,
            on_open_note: Rc::new(on_open_note),
            on_place_inbox_note: Rc::new(on_place_inbox_note),
        };

        // ── Wire scope buttons ─────────────────────────────────────────────
        shell.set_scope_button_active(&initial_scope);
        shell.connect_scope_buttons(content_stack.clone());

        // ── Wire filter chips ──────────────────────────────────────────────
        shell.connect_filter_chips(content_stack.clone());

        // ── Wire search entry ──────────────────────────────────────────────
        {
            let shell2 = shell.clone();
            let stack2 = content_stack.clone();
            search_entry.connect_changed(move |entry| {
                let text = entry.text().to_string();
                shell2.run_and_display(&text, &stack2);
            });
        }

        // ── Clear button ───────────────────────────────────────────────────
        {
            let entry = search_entry.clone();
            clear_btn.connect_clicked(move |_| {
                entry.set_text("");
            });
        }

        // ── Keyboard: Enter in results opens the selected note ─────────────
        {
            let shell2 = shell.clone();
            let key_ctrl = gtk::EventControllerKey::new();
            key_ctrl.connect_key_pressed(move |_, key, _, _| {
                if key == gtk::gdk::Key::Return || key == gtk::gdk::Key::KP_Enter {
                    shell2.open_selected();
                    return gtk::glib::Propagation::Stop;
                }
                gtk::glib::Propagation::Proceed
            });
            shell.results_list.add_controller(key_ctrl);
        }

        shell
    }

    // ── Public API ─────────────────────────────────────────────────────────────

    /// Called when Search Mode becomes the active mode.
    /// Re-evaluates default scope and focuses the search entry.
    pub fn activate(&self) {
        let has_ws = self.workspace_db.borrow().is_some();
        let scope = default_scope(has_ws);
        self.set_scope(&scope);
        self.search_entry.grab_focus();
    }

    /// Pre-fill the query and immediately run a search.
    pub fn set_query_and_run(&self, query: &str) {
        // Connect changed fires automatically on set_text.
        self.search_entry.set_text(query);
        self.search_entry.grab_focus();
        // Move cursor to end.
        let len = query.len() as i32;
        self.search_entry.select_region(len, len);
    }

    // ── Internal helpers ───────────────────────────────────────────────────────

    fn set_scope(&self, scope: &SearchScope) {
        *self.current_scope.borrow_mut() = scope.clone();
        self.set_scope_button_active(scope);
    }

    fn set_scope_button_active(&self, scope: &SearchScope) {
        // Block signals to avoid recursive refresh triggers.
        self.scope_inbox_btn
            .set_active(matches!(scope, SearchScope::Inbox));
        self.scope_ws_btn
            .set_active(matches!(scope, SearchScope::CurrentWorkspace));
        self.scope_both_btn
            .set_active(matches!(scope, SearchScope::CurrentWorkspaceAndInbox));
        self.scope_all_btn.set_active(matches!(
            scope,
            SearchScope::AllKnownWorkspaces | SearchScope::AllKnownWorkspacesAndInbox
        ));
    }

    fn connect_scope_buttons(&self, stack: gtk::Stack) {
        macro_rules! scope_connect {
            ($btn:expr, $scope:expr) => {{
                let shell = self.clone();
                let stack = stack.clone();
                $btn.connect_toggled(move |btn| {
                    if btn.is_active() {
                        shell.set_scope_exclusive($scope, &stack);
                    }
                });
            }};
        }
        scope_connect!(self.scope_inbox_btn, SearchScope::Inbox);
        scope_connect!(self.scope_ws_btn, SearchScope::CurrentWorkspace);
        scope_connect!(self.scope_both_btn, SearchScope::CurrentWorkspaceAndInbox);
        {
            let shell = self.clone();
            let stack = stack.clone();
            self.scope_all_btn.connect_toggled(move |btn| {
                if btn.is_active() {
                    let include_inbox = shell.inbox_db.borrow().is_some();
                    shell.set_scope_exclusive(SearchScope::all_known(include_inbox), &stack);
                }
            });
        }
    }

    fn set_scope_exclusive(&self, scope: SearchScope, stack: &gtk::Stack) {
        // Deactivate other buttons without triggering their toggled signals
        // (GTK toggle buttons don't have a group mechanism outside ToggleButton
        // subclasses, so we set_active without retriggering by checking current).
        let all = [
            (&self.scope_inbox_btn, SearchScope::Inbox),
            (&self.scope_ws_btn, SearchScope::CurrentWorkspace),
            (&self.scope_both_btn, SearchScope::CurrentWorkspaceAndInbox),
            (&self.scope_all_btn, SearchScope::AllKnownWorkspacesAndInbox),
        ];
        for (btn, s) in &all {
            let should_be_active = std::mem::discriminant(s) == std::mem::discriminant(&scope);
            if btn.is_active() != should_be_active {
                btn.set_active(should_be_active);
            }
        }
        *self.current_scope.borrow_mut() = scope;
        let query = self.search_entry.text().to_string();
        self.run_and_display(&query, stack);
    }

    fn connect_filter_chips(&self, stack: gtk::Stack) {
        macro_rules! filter_connect {
            ($chip:expr) => {{
                let shell = self.clone();
                let stack = stack.clone();
                $chip.connect_toggled(move |_| {
                    let query = shell.search_entry.text().to_string();
                    shell.run_and_display(&query, &stack);
                });
            }};
        }
        filter_connect!(self.filter_pinned);
        filter_connect!(self.filter_checklist);
        filter_connect!(self.filter_image);
        filter_connect!(self.filter_links);
    }

    fn current_filters(&self) -> SearchFilters {
        SearchFilters {
            pinned_only: self.filter_pinned.is_active(),
            has_checklist: self.filter_checklist.is_active(),
            has_image: self.filter_image.is_active(),
            has_links: self.filter_links.is_active(),
        }
    }

    fn run_and_display(&self, raw_query: &str, stack: &gtk::Stack) {
        let query = SearchQuery::parse(raw_query);
        let scope = self.current_scope.borrow().clone();
        let filters = self.current_filters();

        let inbox_ref = self.inbox_db.borrow();
        let ws_ref = self.workspace_db.borrow();
        let kw_ref = self.known_ws.borrow();

        let output = run_search(
            &query,
            &scope,
            &filters,
            inbox_ref.as_ref(),
            ws_ref.as_ref(),
            &kw_ref,
        );
        drop(inbox_ref);
        drop(ws_ref);
        drop(kw_ref);

        self.display_results(output.results, &query.raw, scope.display_label(), stack);

        if output.unavailable_workspaces.is_empty() {
            self.unavailable_label.set_visible(false);
        } else {
            let count = output.unavailable_workspaces.len();
            self.unavailable_label.set_text(&format!(
                "⚠ {count} workspace{} unavailable",
                if count == 1 { "" } else { "s" }
            ));
            self.unavailable_label.set_visible(true);
        }
    }

    fn display_results(
        &self,
        results: Vec<SearchResult>,
        raw_query: &str,
        scope_label: &str,
        stack: &gtk::Stack,
    ) {
        // Clear existing rows.
        while let Some(child) = self.results_list.first_child() {
            self.results_list.remove(&child);
        }

        if results.is_empty() {
            let msg = if raw_query.trim().is_empty() {
                "Type to search. Results appear here.".to_string()
            } else {
                format!(
                    "No notes match \"{raw_query}\".\nTry a different term or broaden the scope."
                )
            };
            self.empty_label.set_text(&msg);
            stack.set_visible_child_name("empty");
            self.status_label.set_text("");
            return;
        }

        let count = results.len();
        let count_label = if raw_query.trim().is_empty() {
            format!(
                "{count} recent note{} in {scope_label}",
                if count == 1 { "" } else { "s" }
            )
        } else {
            format!(
                "{count} result{} for \"{raw_query}\" in {scope_label}",
                if count == 1 { "" } else { "s" }
            )
        };
        self.status_label.set_text(&count_label);

        for (i, result) in results.iter().enumerate() {
            let row = self.build_result_row(result);
            // Cascade the first several rows in for a gentle "beat" as results
            // land, instead of snapping in all at once. Later rows appear plain
            // (and are likely off-screen anyway).
            row.add_css_class("blot-card-anim");
            if i < 8 {
                row.add_css_class(&format!("blot-delay-{i}"));
            }
            self.results_list.append(&row);
        }

        stack.set_visible_child_name("results");
    }

    fn build_result_row(&self, result: &SearchResult) -> gtk::ListBoxRow {
        let row = gtk::ListBoxRow::new();
        row.add_css_class("search-result-row");

        let card = gtk::Box::new(gtk::Orientation::Vertical, 2);
        card.add_css_class("search-result-card");
        card.set_margin_top(6);
        card.set_margin_bottom(6);
        card.set_margin_start(12);
        card.set_margin_end(12);

        // ── Row 1: title + date + pin ──────────────────────────────────────
        let row1 = gtk::Box::new(gtk::Orientation::Horizontal, 6);

        let title_label = gtk::Label::new(Some(&result.title));
        title_label.add_css_class("result-title");
        title_label.set_halign(gtk::Align::Start);
        title_label.set_hexpand(true);
        title_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        title_label.set_xalign(0.0);

        let meta_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        meta_box.set_halign(gtk::Align::End);

        // Indicators
        if result.is_pinned {
            let pin = gtk::Label::new(Some("★"));
            pin.add_css_class("result-pin-indicator");
            meta_box.append(&pin);
        }
        if result.has_checklist {
            let ic = gtk::Label::new(Some("✓"));
            ic.add_css_class("result-indicator");
            ic.set_tooltip_text(Some("Has checklist"));
            meta_box.append(&ic);
        }
        if result.has_image {
            let ic = gtk::Label::new(Some("◻"));
            ic.add_css_class("result-indicator");
            ic.set_tooltip_text(Some("Has image"));
            meta_box.append(&ic);
        }
        if result.has_links {
            let ic = gtk::Label::new(Some("↗"));
            ic.add_css_class("result-indicator");
            ic.set_tooltip_text(Some("Has links"));
            meta_box.append(&ic);
        }

        // Source kind chip
        let kind_chip = gtk::Label::new(Some(match result.source_kind {
            crate::search::NoteSourceKind::InboxNote => "Inbox",
            crate::search::NoteSourceKind::WorkspaceNote => "WS",
        }));
        kind_chip.add_css_class("result-source-chip");
        meta_box.append(&kind_chip);

        // Date (show last 10 chars of ISO string = YYYY-MM-DD)
        let date_str = if result.updated_at.len() >= 10 {
            &result.updated_at[..10]
        } else {
            &result.updated_at
        };
        let date_label = gtk::Label::new(Some(date_str));
        date_label.add_css_class("result-date");
        meta_box.append(&date_label);

        row1.append(&title_label);
        row1.append(&meta_box);

        // ── Row 2: snippet ─────────────────────────────────────────────────
        let snippet_label = gtk::Label::new(Some(if result.snippet.is_empty() {
            "(no preview)"
        } else {
            &result.snippet
        }));
        snippet_label.add_css_class("result-snippet");
        snippet_label.set_halign(gtk::Align::Start);
        snippet_label.set_hexpand(true);
        snippet_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        snippet_label.set_xalign(0.0);
        snippet_label.set_max_width_chars(120);

        // ── Row 3: location + open button + (inbox only) place button ─────
        let row3 = gtk::Box::new(gtk::Orientation::Horizontal, 6);

        let loc_label = gtk::Label::new(Some(&result.full_location_label()));
        loc_label.add_css_class("result-location");
        loc_label.set_halign(gtk::Align::Start);
        loc_label.set_hexpand(true);
        loc_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        loc_label.set_xalign(0.0);

        let open_btn = gtk::Button::with_label("Open");
        open_btn.add_css_class("result-open-btn");
        open_btn.add_css_class("suggested-action");

        row3.append(&loc_label);

        // "Place" button only for Inbox notes (not already-placed workspace notes).
        if matches!(result.source_kind, crate::search::NoteSourceKind::InboxNote) {
            let place_btn = gtk::Button::with_label("Place…");
            place_btn.add_css_class("result-place-btn");
            place_btn.set_tooltip_text(Some("Move this note into a workspace"));
            let cb = self.on_place_inbox_note.clone();
            let note_id = result.note_id.clone();
            let note_title = result.title.clone();
            place_btn.connect_clicked(move |_| (cb)(note_id.clone(), note_title.clone()));
            row3.append(&place_btn);
        }

        row3.append(&open_btn);

        card.append(&row1);
        card.append(&snippet_label);
        card.append(&row3);
        row.set_child(Some(&card));

        // ── Open button action ─────────────────────────────────────────────
        let target = self.result_to_target(result);
        let on_open = self.on_open_note.clone();
        open_btn.connect_clicked(move |_| {
            on_open(target.clone());
        });

        row
    }

    fn result_to_target(&self, result: &SearchResult) -> SearchNoteTarget {
        match result.source_kind {
            crate::search::NoteSourceKind::InboxNote => SearchNoteTarget::InboxNote {
                note_id: result.note_id.clone(),
            },
            crate::search::NoteSourceKind::WorkspaceNote => SearchNoteTarget::WorkspaceNote {
                note_id: result.note_id.clone(),
                workspace_path: result
                    .workspace_path
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("")),
            },
        }
    }

    fn open_selected(&self) {
        let Some(row) = self.results_list.selected_row() else {
            return;
        };
        // Find and click the "Open" button inside the selected row.
        if let Some(card) = row.child() {
            find_and_click_open(&card);
        }
    }
}

/// Recursively walk widget children to find and activate the first "Open" button.
fn find_and_click_open(widget: &gtk::Widget) {
    if let Some(btn) = widget.downcast_ref::<gtk::Button>() {
        if btn.label().as_deref() == Some("Open") {
            btn.emit_clicked();
            return;
        }
    }
    let mut child = widget.first_child();
    while let Some(c) = child {
        find_and_click_open(&c);
        child = c.next_sibling();
    }
}
