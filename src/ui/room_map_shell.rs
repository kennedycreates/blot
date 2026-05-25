use gtk::prelude::*;

/// Build the Room Map Mode placeholder surface.
///
/// Room Map Mode shows Rooms and Doors in both a canvas view and a
/// list/sidebar view. Rooms can be dragged on the canvas; Doors show
/// connection type (normal, strong, weak). Implemented in a later prompt.
pub fn build() -> gtk::Box {
    let container = gtk::Box::new(gtk::Orientation::Vertical, 24);
    container.add_css_class("shell-placeholder");
    container.set_halign(gtk::Align::Center);
    container.set_valign(gtk::Align::Center);
    container.set_vexpand(true);
    container.set_hexpand(true);

    let icon = gtk::Label::new(Some("⬡"));
    icon.add_css_class("placeholder-icon");

    let title = gtk::Label::new(Some("Room Map"));
    title.add_css_class("placeholder-title");

    let desc = gtk::Label::new(Some(
        "Visual and list view of Rooms and their Doors.\nComing in a later prompt.",
    ));
    desc.add_css_class("placeholder-desc");
    desc.set_justify(gtk::Justification::Center);

    container.append(&icon);
    container.append(&title);
    container.append(&desc);
    container
}
