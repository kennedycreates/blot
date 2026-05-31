use crate::config::AppConfig;
use crate::inbox::InboxDb;
use crate::launch::LaunchMode;
use crate::paths::AppPaths;
use crate::ui::main_window::MainWindow;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, CssProvider};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

pub fn on_activate(app: &Application, launch: &LaunchMode) {
    let paths = Rc::new(AppPaths::resolve());
    paths.ensure_dirs();

    let config = Rc::new(AppConfig::load(&paths));
    load_css(&config.theme, &paths);

    if cfg!(debug_assertions) {
        install_dev_assets();
    }

    // Open (or create) the Global Inbox database.
    let db: Rc<RefCell<Option<InboxDb>>> = match InboxDb::open(&paths.inbox_db) {
        Ok(db) => {
            eprintln!("blot: inbox opened at {}", paths.inbox_db.display());
            Rc::new(RefCell::new(Some(db)))
        }
        Err(e) => {
            eprintln!(
                "blot: failed to open inbox at {}: {e}",
                paths.inbox_db.display()
            );
            Rc::new(RefCell::new(None))
        }
    };

    let window = MainWindow::new(app, launch.config(), config, paths, db);
    setup_icon(&window);
    window.present();
}

/// Open a fresh Blot window sharing the same Inbox database.
/// Call this to implement "Open in New Window".
pub fn open_new_window(
    app: &Application,
    db: Rc<RefCell<Option<InboxDb>>>,
    config: Rc<AppConfig>,
    paths: Rc<AppPaths>,
) -> ApplicationWindow {
    use crate::launch::LaunchConfig;
    let launch = LaunchConfig::default();
    let window = MainWindow::new(app, &launch, config, paths, db);
    setup_icon(&window);
    window.present();
    window
}

fn load_css(theme: &str, paths: &AppPaths) {
    let provider = CssProvider::new();

    if let Some(path) = find_theme(theme, paths) {
        provider.load_from_path(&path);
    } else if theme != "default" {
        eprintln!("blot: theme '{theme}' not found, falling back to default");
        if let Some(path) = find_theme("default", paths) {
            provider.load_from_path(&path);
        }
    }

    let Some(display) = gtk::gdk::Display::default() else {
        eprintln!("blot: no display available for CSS loading");
        return;
    };

    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

/// Resolve a theme CSS file by name. Resolution order:
/// 1. `~/.config/blot/themes/<name>.css` — user override
/// 2. `themes/<name>.css` relative to CWD — dev / run-in-place
/// 3. `<exe>/../../../themes/<name>.css` — cargo build layout
/// 4. `<exe>/../share/blot/themes/<name>.css` — installed layout
fn find_theme(theme: &str, paths: &AppPaths) -> Option<PathBuf> {
    let filename = format!("{theme}.css");

    let user = paths.themes_dir.join(&filename);
    if user.exists() {
        return Some(user);
    }

    let cwd = PathBuf::from("themes").join(&filename);
    if cwd.exists() {
        return Some(cwd);
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dev) = exe
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .map(|p| p.join("themes").join(&filename))
        {
            if dev.exists() {
                return Some(dev);
            }
        }
        if let Some(inst) = exe
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("share").join("blot").join("themes").join(&filename))
        {
            if inst.exists() {
                return Some(inst);
            }
        }
    }

    None
}

fn setup_icon(window: &ApplicationWindow) {
    if let Some(icons_dir) = find_icons_dir() {
        if let Some(display) = gtk::gdk::Display::default() {
            let icon_theme = gtk::IconTheme::for_display(&display);
            icon_theme.add_search_path(icons_dir.to_string_lossy().as_ref());
        }
    }
    window.set_icon_name(Some("blot"));
}

/// Resolve the icons directory using the same search order as `find_theme`.
fn find_icons_dir() -> Option<PathBuf> {
    let cwd = PathBuf::from("icons");
    if cwd.exists() {
        return cwd.canonicalize().ok().or(Some(cwd));
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dev) = exe
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .map(|p| p.join("icons"))
        {
            if dev.exists() {
                return Some(dev);
            }
        }
        if let Some(inst) = exe
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("share").join("blot").join("icons"))
        {
            if inst.exists() {
                return Some(inst);
            }
        }
    }

    None
}

/// In debug builds, copy the desktop file to the user's local applications
/// directory so the app can be launched from the system application menu.
fn install_dev_assets() {
    let Some(desktop_src) = find_desktop_file() else {
        return;
    };
    let apps_dir = glib::user_data_dir().join("applications");
    if std::fs::create_dir_all(&apps_dir).is_ok() {
        let dest = apps_dir.join("com.watercolor.Blot.desktop");
        let needs_update = !dest.exists()
            || desktop_src.metadata().ok().and_then(|m| m.modified().ok())
                > dest.metadata().ok().and_then(|m| m.modified().ok());
        if needs_update {
            let _ = std::fs::copy(&desktop_src, &dest);
        }
    }
}

fn find_desktop_file() -> Option<PathBuf> {
    const FILENAME: &str = "com.watercolor.Blot.desktop";

    let cwd = PathBuf::from(FILENAME);
    if cwd.exists() {
        return Some(cwd);
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dev) = exe
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .map(|p| p.join(FILENAME))
        {
            if dev.exists() {
                return Some(dev);
            }
        }
        if let Some(inst) = exe
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("share").join("blot").join(FILENAME))
        {
            if inst.exists() {
                return Some(inst);
            }
        }
    }

    None
}
