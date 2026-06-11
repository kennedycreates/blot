mod absorb;
mod app;
mod config;
mod document;
mod external_file;
mod inbox;
mod known_workspaces;
mod launch;
mod note_version;
mod ops;
mod paths;
mod place_note;
mod search;
mod title;
mod ui;
#[allow(dead_code)]
mod water_file;
mod workspace;

use gtk::prelude::*;
use gtk::Application;

const APP_ID: &str = "com.watercolor.Blot";

fn main() -> glib::ExitCode {
    glib::set_application_name("Blot");
    let launch_mode = launch::LaunchMode::from_env();
    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(move |app| app::on_activate(app, &launch_mode));
    // Pass only the binary name so our custom flags don't trigger
    // GLib's unknown-argument warnings.
    let prog = std::env::args().next().unwrap_or_default();
    app.run_with_args(&[prog])
}
