use adw::prelude::*;
use ashpd::desktop::background::Background;
use gtk::glib;
use ksni::blocking::TrayMethods;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime};
use yanklog_core::{
    check_for_update, copy_to_clipboard, install_profile_update, profile_release_notes,
    ClipboardMonitor, Config, Database, Platform, Profile, ThemePreference,
};

const DIRECT_APP_ID: &str = "com.yanklog.app";
const FLATPAK_APP_ID: &str = "com.yanklog.YankLog";
const BUNDLED_LINUX_INSTALL_SCRIPT: &str = include_str!("../../../install.sh");
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const QUICK_PICKER_WIDTH: i32 = 560;
const QUICK_PICKER_HEIGHT: i32 = 420;
const HISTORY_PAGE_SIZE: usize = 100;

fn is_flatpak_build() -> bool {
    cfg!(feature = "flatpak")
}

fn app_id() -> &'static str {
    if is_flatpak_build() {
        FLATPAK_APP_ID
    } else {
        DIRECT_APP_ID
    }
}

fn picker_app_id() -> &'static str {
    if is_flatpak_build() {
        "com.yanklog.YankLog.Picker"
    } else {
        "com.yanklog.app.picker"
    }
}

#[derive(Debug, Default)]
struct PauseState {
    paused: std::sync::atomic::AtomicBool,
    until: Mutex<Option<SystemTime>>,
}

impl PauseState {
    fn pause(&self, duration: Option<Duration>) {
        if let Ok(mut until) = self.until.lock() {
            *until = duration.and_then(|duration| SystemTime::now().checked_add(duration));
        }
        self.paused.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn resume(&self) {
        if let Ok(mut until) = self.until.lock() {
            *until = None;
        }
        self.paused
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    fn is_paused(&self) -> bool {
        if !self.paused.load(std::sync::atomic::Ordering::SeqCst) {
            return false;
        }
        let expired = self
            .until
            .lock()
            .ok()
            .and_then(|until| *until)
            .is_some_and(|until| SystemTime::now() >= until);
        if expired {
            self.resume();
            return false;
        }
        true
    }

    fn monitoring_label(&self) -> String {
        if !self.is_paused() {
            return "Monitoring clipboard".to_string();
        }
        let remaining = self
            .until
            .lock()
            .ok()
            .and_then(|until| *until)
            .and_then(|until| until.duration_since(SystemTime::now()).ok());
        match remaining {
            Some(duration) => {
                let minutes = duration.as_secs().div_ceil(60).max(1);
                format!("Monitoring paused · {minutes} min remaining")
            }
            None => "Monitoring paused".to_string(),
        }
    }
}
const YANKLOG_CSS: &str = r#"
window {
  background: @window_bg_color;
  color: @window_fg_color;
}

headerbar {
  background: @window_bg_color;
  color: @window_fg_color;
  border-bottom: 0;
  box-shadow: none;
  min-height: 34px;
  padding: 0 6px;
}

button {
  min-height: 38px;
  padding: 0 18px;
  border-radius: 8px;
  color: @window_fg_color;
  background: @view_bg_color;
  border: 1px solid @borders;
  box-shadow: none;
}

button:hover {
  background: @card_bg_color;
}

button.destructive-action {
  color: @error_color;
  border-color: alpha(@error_color, 0.45);
}

headerbar button,
headerbar button:hover,
headerbar button:active {
  min-height: 24px;
  min-width: 24px;
  padding: 0;
  border-radius: 999px;
  background: transparent;
  border: 0;
  box-shadow: none;
}

headerbar button:hover {
  background: @card_bg_color;
}

entry {
  min-height: 46px;
  border-radius: 8px;
  color: @window_fg_color;
  background: @view_bg_color;
  border: 1px solid @borders;
  box-shadow: none;
}

entry:focus,
.search-entry:focus {
  border-color: @accent_color;
  box-shadow: none;
}

.app-root {
  background: @window_bg_color;
}

window.quick-picker-window {
  background: transparent;
}

.quick-picker-root {
  background: @window_bg_color;
  border-radius: 8px;
  padding: 12px;
}

.content-header {
  margin-bottom: 12px;
}

.app-title {
  color: @window_fg_color;
  font-size: 34px;
  font-weight: 800;
}

.monitoring {
  color: @success_color;
  font-size: 16px;
  font-weight: 600;
}

.muted {
  color: alpha(@window_fg_color, 0.68);
}

.quick-picker-hint {
  color: alpha(@window_fg_color, 0.68);
  font-size: 12px;
}

.privacy-card {
  min-height: 52px;
  padding: 0 14px;
  border-radius: 8px;
  background: alpha(@success_color, 0.16);
  border: 1px solid alpha(@success_color, 0.72);
}

.privacy-title {
  color: @success_color;
  font-weight: 700;
}

.history-list {
  background: @view_bg_color;
  border: 1px solid @borders;
  border-radius: 8px;
}

.history-row {
  background: @view_bg_color;
  color: @window_fg_color;
  border-bottom: 1px solid @borders;
}

.history-row:hover {
  background: @card_bg_color;
}

.history-row.quick-selected,
.history-row:selected {
  background: alpha(@accent_color, 0.22);
}

.history-row.quick-selected .row-preview,
.history-row:selected .row-preview {
  color: @window_fg_color;
}

.row-preview {
  color: @window_fg_color;
  font-size: 16px;
}

.row-meta {
  color: alpha(@window_fg_color, 0.68);
  font-size: 13px;
}

.entry-action {
  min-height: 30px;
  padding: 0 10px;
  font-size: 12px;
}

.entry-actions {
  opacity: 0;
}

.history-row:hover .entry-actions,
.history-row:selected .entry-actions {
  opacity: 1;
}

.history-footer {
  margin-top: 4px;
}

.empty-state {
  color: @window_fg_color;
  font-size: 18px;
  font-weight: 700;
}

.settings-panel {
  padding: 16px;
  border-radius: 8px;
  background: @view_bg_color;
  border: 1px solid @borders;
}

.settings-note {
  color: alpha(@window_fg_color, 0.68);
  font-size: 13px;
}

.status-toast {
  color: @success_color;
  font-size: 13px;
  font-weight: 600;
}
"#;

pub fn run() {
    let start_background = std::env::args().any(|arg| arg == "--background");
    let start_hidden = std::env::args().any(|arg| arg == "--hidden") || start_background;

    if std::env::args().any(|arg| arg == "--version") {
        println!("{APP_VERSION}");
        return;
    }

    if std::env::args().any(|arg| arg == "--update") {
        run_cli_update();
        return;
    }

    if std::env::args().any(|arg| arg == "--pick" || arg == "-p") {
        run_quick_picker();
        return;
    }

    let profile = profile();
    let config = Config::load(&profile).unwrap_or_default();

    if start_background {
        run_background_mode(&profile, config);
        return;
    }

    let application = adw::Application::builder().application_id(app_id()).build();
    application.connect_activate(move |app| {
        install_css();
        apply_theme(&Config::load(&profile).unwrap_or_default());
        build_main_window(app, !start_hidden);
    });
    application.run();
}

fn run_background_mode(profile: &Profile, config: Config) {
    let database = match open_database_with_secure_key(profile) {
        Ok(database) => Arc::new(Mutex::new(database)),
        Err(err) => {
            eprintln!("Failed to open yanklog database: {err}");
            std::process::exit(1);
        }
    };
    let pause_state = Arc::new(PauseState::default());
    let monitor = ClipboardMonitor::new(config.poll_interval_ms);

    start_clipboard_monitor(
        Arc::clone(&database),
        monitor,
        config,
        Arc::clone(&pause_state),
    );

    let (sender, receiver) = mpsc::channel();
    let tray = YanklogTray {
        sender,
        update_available: None,
        pause_state: Arc::clone(&pause_state),
    };
    let Ok(handle) = tray
        .assume_sni_available(true)
        .disable_dbus_name(is_flatpak_build())
        .spawn()
    else {
        std::thread::park();
        return;
    };
    std::mem::forget(handle);

    loop {
        while let Ok(command) = receiver.try_recv() {
            match command {
                TrayCommand::Show => {
                    let _ = Command::new(std::env::current_exe().unwrap())
                        .arg("--hidden")
                        .spawn();
                }
                TrayCommand::QuickPick => {
                    let _ = Command::new(std::env::current_exe().unwrap())
                        .arg("--pick")
                        .spawn();
                }
                TrayCommand::PauseFor(seconds) => {
                    pause_state.pause(seconds.map(Duration::from_secs));
                }
                TrayCommand::PauseUntilTomorrow => {
                    pause_state.pause(pause_until_tomorrow_duration());
                }
                TrayCommand::Resume => {
                    pause_state.resume();
                }
                TrayCommand::CheckUpdate | TrayCommand::Settings => {}
                TrayCommand::Quit => std::process::exit(0),
            }
        }
        thread::sleep(Duration::from_millis(200));
    }
}

fn install_css() {
    let Some(display) = gtk::gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_data(YANKLOG_CSS);
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn apply_theme(config: &Config) {
    let manager = adw::StyleManager::default();
    manager.set_color_scheme(match config.theme {
        ThemePreference::System => adw::ColorScheme::Default,
        ThemePreference::Light => adw::ColorScheme::ForceLight,
        ThemePreference::Dark => adw::ColorScheme::ForceDark,
    });
}

fn profile() -> Profile {
    Profile::new(
        Platform::Linux,
        std::env::var("YANKLOG_DEV_MODE").as_deref() == Ok("1"),
    )
}

fn run_cli_update() {
    if is_flatpak_build() {
        println!("Updates for this installation are managed by Flatpak. Run `flatpak update com.yanklog.YankLog`.");
        return;
    }

    let profile = profile();
    if profile.dev {
        eprintln!("Updates are disabled for yanklog dev builds.");
        std::process::exit(2);
    }

    println!("Checking for yanklog updates...");
    let latest_version = match check_for_update(&profile, APP_VERSION) {
        Ok(Some(version)) => version,
        Ok(None) => {
            println!("yanklog is already up to date ({APP_VERSION}).");
            return;
        }
        Err(err) => {
            eprintln!("Update check failed: {err}");
            std::process::exit(1);
        }
    };

    match profile_release_notes(&profile, &latest_version) {
        Ok(Some(notes)) => println!("Release notes for {latest_version}:\n\n{notes}\n"),
        Ok(None) => println!("No release notes available for {latest_version}.\n"),
        Err(err) => eprintln!("Could not load release notes: {err}"),
    }

    println!("Installing yanklog {latest_version}...");
    match install_profile_update(&profile, &latest_version, BUNDLED_LINUX_INSTALL_SCRIPT) {
        Ok(output) => {
            if !output.trim().is_empty() {
                println!("{}", output.trim());
            }
            println!("Updated yanklog to {latest_version}.");
        }
        Err(err) => {
            eprintln!("Update failed: {err}");
            std::process::exit(1);
        }
    }
}

fn build_main_window(app: &adw::Application, present_window: bool) {
    let profile = profile();
    let database = match open_database_with_secure_key(&profile) {
        Ok(database) => Arc::new(Mutex::new(database)),
        Err(err) => {
            show_error_dialog(None, &format!("Failed to open yanklog database: {err}"));
            return;
        }
    };
    let config = Arc::new(Mutex::new(Config::load(&profile).unwrap_or_default()));
    let config_snapshot = config
        .lock()
        .map(|config| config.clone())
        .unwrap_or_default();
    let monitor = ClipboardMonitor::new(config_snapshot.poll_interval_ms);
    let pause_state = Arc::new(PauseState::default());

    start_clipboard_monitor(
        Arc::clone(&database),
        monitor,
        config_snapshot,
        Arc::clone(&pause_state),
    );

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title(profile.display_name())
        .default_width(940)
        .default_height(760)
        .build();

    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
    let settings_button = gtk::Button::with_label("Settings");
    let pause_button = gtk::Button::with_label("Pause");
    let pause_options_button = gtk::MenuButton::new();
    pause_options_button.set_icon_name("pan-down-symbolic");
    pause_options_button.set_tooltip_text(Some("Timed pause options"));

    let search_entry = gtk::SearchEntry::builder()
        .placeholder_text("Search clipboard history")
        .build();
    search_entry.add_css_class("search-entry");

    let title_label = gtk::Label::new(Some(profile.display_name()));
    title_label.set_xalign(0.0);
    title_label.add_css_class("app-title");

    let monitoring_label = gtk::Label::new(Some("Monitoring clipboard"));
    monitoring_label.set_xalign(0.0);
    monitoring_label.add_css_class("monitoring");

    let title_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
    title_box.append(&title_label);
    title_box.append(&monitoring_label);

    let action_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    action_row.set_halign(gtk::Align::End);
    action_row.append(&pause_button);
    action_row.append(&pause_options_button);
    action_row.append(&settings_button);

    let content_header = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    content_header.add_css_class("content-header");
    content_header.append(&title_box);
    content_header.append(&action_row);
    title_box.set_hexpand(true);

    let privacy_title = gtk::Label::new(Some("Private local history"));
    privacy_title.add_css_class("privacy-title");
    let privacy_copy = gtk::Label::new(Some("Entries stay encrypted on this device."));
    privacy_copy.add_css_class("muted");
    let privacy_card = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    privacy_card.add_css_class("privacy-card");
    privacy_card.append(&privacy_title);
    privacy_card.append(&privacy_copy);

    let action_status = gtk::Label::new(None);
    action_status.set_xalign(0.0);
    action_status.add_css_class("status-toast");
    let undo_button = gtk::Button::with_label("Undo");
    undo_button.add_css_class("entry-action");
    undo_button.set_visible(false);
    let status_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    status_row.append(&action_status);
    status_row.append(&undo_button);

    let pause_popover = build_pause_popover(
        Arc::clone(&pause_state),
        &pause_button,
        &monitoring_label,
        &action_status,
    );
    pause_options_button.set_popover(Some(&pause_popover));

    let result_label = gtk::Label::new(None);
    result_label.set_xalign(1.0);
    result_label.add_css_class("muted");
    let search_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    search_entry.set_hexpand(true);
    search_row.append(&search_entry);
    search_row.append(&result_label);

    let list = gtk::ListBox::new();
    list.add_css_class("history-list");
    let scroller = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .child(&list)
        .build();

    let clear_button = gtk::Button::with_label("Clear all");
    clear_button.add_css_class("destructive-action");
    let previous_button = gtk::Button::with_label("Previous");
    let next_button = gtk::Button::with_label("Next");
    let footer_spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    footer_spacer.set_hexpand(true);
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    footer.add_css_class("history-footer");
    footer.append(&clear_button);
    footer.append(&footer_spacer);
    footer.append(&previous_button);
    footer.append(&next_button);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.add_css_class("app-root");
    content.set_margin_top(28);
    content.set_margin_bottom(28);
    content.set_margin_start(28);
    content.set_margin_end(28);
    content.append(&content_header);
    content.append(&privacy_card);
    content.append(&status_row);
    content.append(&search_row);
    content.append(&scroller);
    content.append(&footer);

    window.set_titlebar(Some(&header));
    window.set_child(Some(&content));

    let state = Rc::new(AppState {
        database,
        config,
        list,
        search_entry,
        status_label: action_status.clone(),
        result_label,
        previous_button: previous_button.clone(),
        next_button: next_button.clone(),
        page_offset: Rc::new(Cell::new(0)),
        search_debounce: Rc::new(Cell::new(0_u64)),
        last_deleted: Rc::new(RefCell::new(None)),
        undo_button: undo_button.clone(),
    });
    refresh_entries(&state, None);

    {
        let state = Rc::clone(&state);
        let search_entry = state.search_entry.clone();
        search_entry.connect_search_changed(move |entry| {
            state.page_offset.set(0);
            let generation = state.search_debounce.get().wrapping_add(1);
            state.search_debounce.set(generation);
            let query = entry.text().to_string();
            let delayed_state = state.clone_for_callbacks();
            let debounce = Rc::clone(&state.search_debounce);
            glib::timeout_add_local_once(Duration::from_millis(220), move || {
                if debounce.get() != generation {
                    return;
                }
                refresh_entries(
                    &delayed_state,
                    if query.trim().is_empty() {
                        None
                    } else {
                        Some(query)
                    },
                );
            });
        });
    }

    {
        let state = Rc::clone(&state);
        let window = window.clone();
        clear_button.connect_clicked(move |_| {
            show_clear_history_confirmation(&window, state.clone_for_callbacks());
        });
    }

    {
        let state = Rc::clone(&state);
        previous_button.connect_clicked(move |_| {
            state
                .page_offset
                .set(state.page_offset.get().saturating_sub(HISTORY_PAGE_SIZE));
            refresh_with_current_query(&state);
        });
    }

    {
        let state = Rc::clone(&state);
        next_button.connect_clicked(move |_| {
            state
                .page_offset
                .set(state.page_offset.get().saturating_add(HISTORY_PAGE_SIZE));
            refresh_with_current_query(&state);
        });
    }

    {
        let state = Rc::clone(&state);
        undo_button.connect_clicked(move |_| restore_last_deleted(&state));
    }

    {
        let pause_state = Arc::clone(&pause_state);
        let monitoring_label = monitoring_label.clone();
        let action_status = action_status.clone();
        pause_button.connect_clicked(move |button| {
            if pause_state.is_paused() {
                pause_state.resume();
                show_status(&action_status, "Monitoring resumed");
            } else {
                pause_state.pause(None);
                show_status(&action_status, "Monitoring paused");
            }
            update_pause_ui(&pause_state, button, &monitoring_label);
        });
    }

    {
        let pause_state = Arc::clone(&pause_state);
        let pause_button = pause_button.clone();
        let monitoring_label = monitoring_label.clone();
        glib::timeout_add_seconds_local(1, move || {
            update_pause_ui(&pause_state, &pause_button, &monitoring_label);
            glib::ControlFlow::Continue
        });
    }

    settings_button.connect_clicked({
        let state = Rc::clone(&state);
        let app = app.clone();
        let profile = profile.clone();
        move |_| show_preferences_window(&app, &profile, Rc::clone(&state))
    });

    setup_tray(
        app,
        &window,
        &pause_button,
        &monitoring_label,
        Arc::clone(&pause_state),
        Rc::clone(&state),
    );

    window.connect_close_request(|window| {
        window.hide();
        glib::Propagation::Stop
    });

    if present_window {
        window.present();
        maybe_show_onboarding(app, &window, &profile, Rc::clone(&state));
    }
}

fn maybe_show_onboarding(
    app: &adw::Application,
    parent: &gtk::ApplicationWindow,
    profile: &Profile,
    state: Rc<AppState>,
) {
    let marker = profile.data_dir().join("onboarding-complete");
    if marker.exists() {
        return;
    }

    let config = state
        .config
        .lock()
        .map(|config| config.clone())
        .unwrap_or_default();
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Welcome to yanklog")
        .modal(true)
        .transient_for(parent)
        .default_width(560)
        .default_height(430)
        .resizable(false)
        .build();
    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::SlideLeftRight);

    let welcome = onboarding_page(
        "Private clipboard history",
        "YankLog keeps your clipboard history encrypted on this computer. Your clipboard content is not transmitted anywhere.",
    );
    let protections = onboarding_page(
        "Choose your protections",
        "Sensitive-looking values can be filtered before they ever reach your history.",
    );
    let ignore_secrets = gtk::CheckButton::with_label("Ignore secret-like values");
    ignore_secrets.set_active(config.privacy.ignore_secret_like);
    let ignore_codes = gtk::CheckButton::with_label("Ignore one-time codes");
    ignore_codes.set_active(config.privacy.ignore_one_time_codes);
    protections.append(&ignore_secrets);
    protections.append(&ignore_codes);

    let ready = onboarding_page(
        "Ready for faster copying",
        "Add the Quick Pick command to your desktop keyboard shortcuts, then copy something to try YankLog.",
    );
    ready.append(&info_row("Quick Pick", &quick_picker_command_text()));
    let copy_command = gtk::Button::with_label("Copy Quick Pick Command");
    copy_command.set_halign(gtk::Align::Start);
    let launch_at_startup = gtk::CheckButton::with_label("Start YankLog when I sign in");
    launch_at_startup.set_active(config.launch_at_startup);
    ready.append(&copy_command);
    ready.append(&launch_at_startup);

    stack.add_named(&welcome, Some("welcome"));
    stack.add_named(&protections, Some("protections"));
    stack.add_named(&ready, Some("ready"));
    stack.set_visible_child_name("welcome");

    let page = Rc::new(Cell::new(0_u8));
    let back = gtk::Button::with_label("Back");
    back.set_sensitive(false);
    let next = gtk::Button::with_label("Continue");
    next.add_css_class("suggested-action");
    let status = gtk::Label::new(None);
    status.set_xalign(0.0);
    status.add_css_class("status-toast");

    {
        let status = status.clone();
        copy_command.connect_clicked(move |_| {
            let _ = copy_to_clipboard(&quick_picker_command_text());
            show_status(&status, "Quick Pick command copied");
        });
    }
    {
        let page = Rc::clone(&page);
        let stack = stack.clone();
        let back_button = back.clone();
        let next_button = next.clone();
        back.connect_clicked(move |_| {
            let next_page = page.get().saturating_sub(1);
            page.set(next_page);
            stack.set_visible_child_name(match next_page {
                0 => "welcome",
                1 => "protections",
                _ => "ready",
            });
            back_button.set_sensitive(next_page > 0);
            next_button.set_label("Continue");
        });
    }
    {
        let page = Rc::clone(&page);
        let stack = stack.clone();
        let back = back.clone();
        let window = window.clone();
        let profile = profile.clone();
        let status = status.clone();
        next.connect_clicked(move |button| {
            if page.get() < 2 {
                let next_page = page.get() + 1;
                page.set(next_page);
                stack.set_visible_child_name(if next_page == 1 {
                    "protections"
                } else {
                    "ready"
                });
                back.set_sensitive(true);
                button.set_label(if next_page == 2 { "Finish" } else { "Continue" });
                return;
            }

            let mut next_config = Config::load(&profile).unwrap_or_default();
            next_config.privacy.ignore_secret_like = ignore_secrets.is_active();
            next_config.privacy.ignore_one_time_codes = ignore_codes.is_active();
            next_config.launch_at_startup = launch_at_startup.is_active();
            if !is_flatpak_build() {
                if let Err(error) = set_launch_at_startup(&profile, next_config.launch_at_startup) {
                    show_status(&status, &error);
                    return;
                }
            }
            if is_flatpak_build() {
                let status = status.clone();
                let window = window.clone();
                let marker = marker.clone();
                let profile = profile.clone();
                let state = Rc::clone(&state);
                let button = button.clone();
                button.set_sensitive(false);
                request_flatpak_launch_at_startup(next_config.launch_at_startup, move |result| {
                    button.set_sensitive(true);
                    match result {
                        Ok(enabled) if enabled == next_config.launch_at_startup => {
                            if let Err(error) = next_config.save_linux_app(&profile) {
                                show_status(&status, &format!("Could not save settings: {error}"));
                                return;
                            }
                            if let Ok(mut shared) = state.config.lock() {
                                *shared = next_config;
                            }
                            let _ = std::fs::create_dir_all(profile.data_dir());
                            if let Err(error) = std::fs::write(&marker, "completed\n") {
                                eprintln!("Could not save onboarding state: {error}");
                            }
                            window.close();
                        }
                        Ok(_) => show_status(
                            &status,
                            "Start at login was not enabled by the desktop portal.",
                        ),
                        Err(error) => show_status(&status, &error),
                    }
                });
                return;
            }
            if let Err(error) = next_config.save_linux_app(&profile) {
                show_status(&status, &format!("Could not save settings: {error}"));
                return;
            }
            if let Ok(mut shared) = state.config.lock() {
                *shared = next_config;
            }
            let _ = std::fs::create_dir_all(profile.data_dir());
            if let Err(error) = std::fs::write(&marker, "completed\n") {
                eprintln!("Could not save onboarding state: {error}");
            }
            window.close();
        });
    }

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    actions.append(&back);
    actions.append(&spacer);
    actions.append(&next);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.add_css_class("app-root");
    content.set_margin_top(24);
    content.set_margin_bottom(24);
    content.set_margin_start(28);
    content.set_margin_end(28);
    content.append(&stack);
    content.append(&status);
    content.append(&actions);
    window.set_child(Some(&content));
    window.present();
}

fn onboarding_page(title: &str, body: &str) -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 14);
    let title = gtk::Label::new(Some(title));
    title.set_xalign(0.0);
    title.add_css_class("app-title");
    let body = gtk::Label::new(Some(body));
    body.set_xalign(0.0);
    body.set_wrap(true);
    body.add_css_class("settings-note");
    page.append(&title);
    page.append(&body);
    page
}

fn open_database_with_secure_key(profile: &Profile) -> Result<Database, String> {
    let key_profile = profile.clone();
    let key_result = thread::spawn(move || resolve_secure_database_key(&key_profile))
        .join()
        .map_err(|_| "The secure-key worker stopped unexpectedly.".to_string())?;

    match key_result {
        Ok((key, legacy_key_path)) => {
            let database = Database::open_with_key(profile.clone(), &key)
                .map_err(|error| error.to_string())?;
            if let Some(path) = legacy_key_path {
                if let Err(error) = std::fs::remove_file(&path) {
                    eprintln!(
                        "Database key migrated to Secret Service, but the legacy key file could not be removed: {error}"
                    );
                }
            }
            Ok(database)
        }
        Err(error) => {
            eprintln!(
                "Secure key storage unavailable; using the protected local key file: {error}"
            );
            Database::open(profile.clone()).map_err(|database_error| database_error.to_string())
        }
    }
}

fn resolve_secure_database_key(profile: &Profile) -> Result<(String, Option<PathBuf>), String> {
    let account = if profile.dev {
        "database-key-dev"
    } else {
        "database-key"
    };
    let entry = keyring::Entry::new(app_id(), account).map_err(|error| error.to_string())?;
    match entry.get_password() {
        Ok(key) if Database::is_valid_encryption_key(key.trim()) => {
            return Ok((key.trim().to_string(), None));
        }
        Ok(_) => return Err("The stored database key is invalid.".to_string()),
        Err(keyring::Error::NoEntry) => {}
        Err(error) => return Err(error.to_string()),
    }

    let legacy_key_path = profile.data_dir().join("secret.key");
    let (key, migrated_path) = if legacy_key_path.exists() {
        let key = std::fs::read_to_string(&legacy_key_path)
            .map_err(|error| format!("Could not read the existing database key: {error}"))?;
        let key = key.trim().to_string();
        if !Database::is_valid_encryption_key(&key) {
            return Err("The existing database key is invalid.".to_string());
        }
        (key, Some(legacy_key_path))
    } else {
        (Database::generate_encryption_key(), None)
    };

    entry
        .set_password(&key)
        .map_err(|error| error.to_string())?;
    Ok((key, migrated_path))
}

fn build_pause_popover(
    pause_state: Arc<PauseState>,
    pause_button: &gtk::Button,
    monitoring_label: &gtk::Label,
    status_label: &gtk::Label,
) -> gtk::Popover {
    let popover = gtk::Popover::new();
    let options = gtk::Box::new(gtk::Orientation::Vertical, 6);
    options.set_margin_top(8);
    options.set_margin_bottom(8);
    options.set_margin_start(8);
    options.set_margin_end(8);

    let choices = [
        ("Pause for 15 minutes", Some(Duration::from_secs(15 * 60))),
        ("Pause for 1 hour", Some(Duration::from_secs(60 * 60))),
        ("Pause until tomorrow", pause_until_tomorrow_duration()),
        ("Pause indefinitely", None),
    ];
    for (label, duration) in choices {
        let button = gtk::Button::with_label(label);
        let pause_state = Arc::clone(&pause_state);
        let pause_button = pause_button.clone();
        let monitoring_label = monitoring_label.clone();
        let status_label = status_label.clone();
        let popover = popover.clone();
        button.connect_clicked(move |_| {
            pause_state.pause(duration);
            update_pause_ui(&pause_state, &pause_button, &monitoring_label);
            show_status(&status_label, label);
            popover.popdown();
        });
        options.append(&button);
    }
    popover.set_child(Some(&options));
    popover
}

fn pause_until_tomorrow_duration() -> Option<Duration> {
    let now = chrono::Local::now();
    let tomorrow = now.date_naive().checked_add_days(chrono::Days::new(1))?;
    let midnight = tomorrow.and_hms_opt(0, 0, 0)?;
    let target = midnight.and_local_timezone(chrono::Local).earliest()?;
    (target - now).to_std().ok()
}

fn update_pause_ui(
    pause_state: &PauseState,
    pause_button: &gtk::Button,
    monitoring_label: &gtk::Label,
) {
    let paused = pause_state.is_paused();
    pause_button.set_label(if paused { "Resume" } else { "Pause" });
    monitoring_label.set_text(&pause_state.monitoring_label());
}

fn setup_tray(
    app: &adw::Application,
    window: &gtk::ApplicationWindow,
    pause_button: &gtk::Button,
    monitoring_label: &gtk::Label,
    pause_state: Arc<PauseState>,
    state: Rc<AppState>,
) {
    let (sender, receiver) = mpsc::channel();
    let tray = YanklogTray {
        sender,
        update_available: None,
        pause_state: Arc::clone(&pause_state),
    };
    let Ok(handle) = tray
        .assume_sni_available(true)
        .disable_dbus_name(is_flatpak_build())
        .spawn()
    else {
        return;
    };
    let update_tray_handle = handle.clone();
    thread::spawn(move || {
        let profile = profile();
        if is_flatpak_build()
            || profile.dev
            || std::env::var("YANKLOG_DISABLE_UPDATE_CHECK").as_deref() == Ok("1")
        {
            return;
        }

        if let Ok(Some(version)) = check_for_update(&profile, APP_VERSION) {
            let skipped = Config::load(&profile)
                .and_then(|config| config.skipped_update_version)
                .is_some_and(|skipped| skipped == version);
            if skipped {
                return;
            }
            let _ = update_tray_handle.update(|tray| {
                tray.update_available = Some(version);
            });
        }
    });

    let app = app.clone();
    let window = window.clone();
    let pause_button = pause_button.clone();
    let monitoring_label = monitoring_label.clone();
    let state = Rc::clone(&state);
    let pause_tray_handle = handle.clone();
    let command_pause_state = Arc::clone(&pause_state);
    glib::timeout_add_local(Duration::from_millis(150), move || {
        while let Ok(command) = receiver.try_recv() {
            match command {
                TrayCommand::Show => {
                    window.present();
                }
                TrayCommand::QuickPick => show_quick_picker_window(&app),
                TrayCommand::CheckUpdate => show_update_status_window(&app),
                TrayCommand::Settings => {
                    show_preferences_window(&app, &profile(), Rc::clone(&state))
                }
                TrayCommand::PauseFor(seconds) => {
                    command_pause_state.pause(seconds.map(Duration::from_secs));
                    update_pause_ui(&command_pause_state, &pause_button, &monitoring_label);
                    let _ = pause_tray_handle.update(|_| {});
                }
                TrayCommand::PauseUntilTomorrow => {
                    command_pause_state.pause(pause_until_tomorrow_duration());
                    update_pause_ui(&command_pause_state, &pause_button, &monitoring_label);
                    let _ = pause_tray_handle.update(|_| {});
                }
                TrayCommand::Resume => {
                    command_pause_state.resume();
                    update_pause_ui(&command_pause_state, &pause_button, &monitoring_label);
                    let _ = pause_tray_handle.update(|_| {});
                }
                TrayCommand::Quit => app.quit(),
            }
        }
        glib::ControlFlow::Continue
    });

    let refresh_tray_handle = handle.clone();
    let last_tray_paused = Cell::new(pause_state.is_paused());
    glib::timeout_add_seconds_local(1, move || {
        let paused = pause_state.is_paused();
        if paused != last_tray_paused.get() {
            last_tray_paused.set(paused);
            let _ = refresh_tray_handle.update(|_| {});
        }
        glib::ControlFlow::Continue
    });

    std::mem::forget(handle);
}

#[derive(Debug)]
enum UpdateStatusResult {
    Available(String, Option<String>),
    Current,
    Disabled,
    Failed(String),
}

fn show_update_status_window(app: &adw::Application) {
    let update_window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Check for update")
        .default_width(460)
        .default_height(320)
        .decorated(true)
        .build();

    let status = gtk::Label::new(Some("Checking for update..."));
    status.set_xalign(0.0);
    status.set_wrap(true);
    status.add_css_class("title-4");

    let release_notes = gtk::Label::new(None);
    release_notes.set_xalign(0.0);
    release_notes.set_yalign(0.0);
    release_notes.set_wrap(true);
    release_notes.set_selectable(true);
    release_notes.set_width_chars(40);
    let notes_scroller = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .hexpand(true)
        .child(&release_notes)
        .build();
    notes_scroller.set_visible(false);

    let install_button = gtk::Button::with_label("Install update");
    install_button.set_visible(false);
    let skip_button = gtk::Button::with_label("Skip this version");
    skip_button.set_visible(false);

    let close_button = gtk::Button::with_label("Close");
    {
        let update_window = update_window.clone();
        close_button.connect_clicked(move |_| update_window.close());
    }

    let button_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    button_row.set_halign(gtk::Align::End);
    button_row.append(&skip_button);
    button_row.append(&install_button);
    button_row.append(&close_button);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(16);
    content.set_margin_end(16);
    content.append(&status);
    content.append(&notes_scroller);
    content.append(&button_row);

    update_window.set_child(Some(&content));
    update_window.present();

    let update_profile = profile();
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = if is_flatpak_build() || update_profile.dev {
            UpdateStatusResult::Disabled
        } else {
            match check_for_update(&update_profile, APP_VERSION) {
                Ok(Some(version)) => {
                    let notes = profile_release_notes(&update_profile, &version).unwrap_or(None);
                    UpdateStatusResult::Available(version, notes)
                }
                Ok(None) => UpdateStatusResult::Current,
                Err(err) => UpdateStatusResult::Failed(err),
            }
        };
        let _ = sender.send(result);
    });

    glib::timeout_add_local(Duration::from_millis(150), move || {
        match receiver.try_recv() {
            Ok(UpdateStatusResult::Available(version, notes)) => {
                status.set_text(&format!("Update available: yanklog {version}"));
                install_button.set_visible(true);
                skip_button.set_visible(true);
                if let Some(notes) = notes {
                    release_notes.set_text(&notes);
                    notes_scroller.set_visible(true);
                } else {
                    release_notes.set_text("Release notes are not available for this version.");
                    notes_scroller.set_visible(true);
                }
                {
                    let version = version.clone();
                    let update_window = update_window.clone();
                    skip_button.connect_clicked(move |_| {
                        let profile = profile();
                        let mut config = Config::load(&profile).unwrap_or_default();
                        config.skipped_update_version = Some(version.clone());
                        let _ = config.save_linux_app(&profile);
                        update_window.close();
                    });
                }
                let install_version = version.clone();
                install_button.connect_clicked({
                    let close_button = close_button.clone();
                    let install_button = install_button.clone();
                    let status = status.clone();
                    let update_window = update_window.clone();
                    move |_| {
                        close_button.set_sensitive(false);
                        install_button.set_sensitive(false);
                        status.set_text(&format!("Installing yanklog {install_version}..."));

                        let profile = profile();
                        let version = install_version.clone();
                        let (sender, receiver) = mpsc::channel();
                        thread::spawn(move || {
                            let result = install_profile_update(
                                &profile,
                                &version,
                                BUNDLED_LINUX_INSTALL_SCRIPT,
                            );
                            let _ = sender.send(result);
                        });

                        glib::timeout_add_local(Duration::from_millis(150), {
                            let close_button = close_button.clone();
                            let install_button = install_button.clone();
                            let status = status.clone();
                            let update_window = update_window.clone();
                            move || match receiver.try_recv() {
                                Ok(Ok(_)) => {
                                    if let Err(err) = relaunch_after_update(&update_window) {
                                        status.set_text(&format!(
                                            "Update installed, but restart failed:\n\n{err}"
                                        ));
                                        close_button.set_sensitive(true);
                                        glib::ControlFlow::Break
                                    } else {
                                        glib::ControlFlow::Break
                                    }
                                }
                                Ok(Err(err)) => {
                                    status.set_text(&format!("Update failed:\n\n{err}"));
                                    close_button.set_sensitive(true);
                                    install_button.set_sensitive(true);
                                    glib::ControlFlow::Break
                                }
                                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                                Err(mpsc::TryRecvError::Disconnected) => {
                                    status.set_text("Update failed.");
                                    close_button.set_sensitive(true);
                                    install_button.set_sensitive(true);
                                    glib::ControlFlow::Break
                                }
                            }
                        });
                    }
                });
                glib::ControlFlow::Break
            }
            Ok(UpdateStatusResult::Current) => {
                status.set_text(&format!("yanklog is already up to date ({APP_VERSION})."));
                glib::ControlFlow::Break
            }
            Ok(UpdateStatusResult::Disabled) => {
                status.set_text(if is_flatpak_build() {
                    "Updates are managed by Flatpak."
                } else {
                    "Updates are disabled for yanklog dev builds."
                });
                glib::ControlFlow::Break
            }
            Ok(UpdateStatusResult::Failed(err)) => {
                status.set_text(&format!("Update check failed:\n\n{err}"));
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                status.set_text("Update check failed.");
                glib::ControlFlow::Break
            }
        }
    });
}

fn relaunch_after_update(window: &gtk::ApplicationWindow) -> Result<(), String> {
    let launcher = linux_launcher_path().ok_or_else(|| "Could not resolve app path".to_string())?;
    let current_pid = std::process::id().to_string();
    Command::new("sh")
        .arg("-c")
        .arg("while kill -0 \"$2\" 2>/dev/null; do sleep 0.1; done; exec \"$1\"")
        .arg("yanklog-relaunch")
        .arg(&launcher)
        .arg(current_pid)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("Failed to relaunch yanklog: {err}"))?;

    if let Some(app) = window.application() {
        app.quit();
    } else {
        window.close();
    }
    std::process::exit(0);
}

fn linux_launcher_path() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok())
}

fn set_launch_at_startup(profile: &Profile, enabled: bool) -> Result<(), String> {
    if is_flatpak_build() {
        return Err("Startup registration is managed outside the Flatpak sandbox.".to_string());
    }

    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| "Could not resolve config directory".to_string())?;
    let autostart_dir = config_home.join("autostart");
    let desktop_file = autostart_dir.join(if profile.dev {
        "com.yanklog.dev.autostart.desktop"
    } else {
        "com.yanklog.app.autostart.desktop"
    });

    if !enabled {
        if desktop_file.exists() {
            std::fs::remove_file(&desktop_file)
                .map_err(|err| format!("Could not remove startup entry: {err}"))?;
        }
        return Ok(());
    }

    let launcher = linux_launcher_path().ok_or_else(|| "Could not resolve app path".to_string())?;
    std::fs::create_dir_all(&autostart_dir)
        .map_err(|err| format!("Could not create startup directory: {err}"))?;
    let name = profile.display_name();
    let exec = desktop_exec_quote(&launcher);
    let content = format!(
        "[Desktop Entry]\nType=Application\nName={name}\nComment=Start {name} at login\nExec={exec} --background\nIcon=com.yanklog.app\nTerminal=false\nX-GNOME-Autostart-enabled=true\nNoDisplay=true\n"
    );
    std::fs::write(&desktop_file, content)
        .map_err(|err| format!("Could not write startup entry: {err}"))?;
    Ok(())
}

fn request_flatpak_launch_at_startup(
    enabled: bool,
    on_complete: impl FnOnce(Result<bool, String>) + 'static,
) {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = async_io::block_on(async move {
            let request = Background::request()
                .reason("Start YankLog at login to monitor your clipboard.")
                .auto_start(enabled)
                .command(["yanklog", "--background"])
                .send()
                .await
                .map_err(|error| format!("Could not request startup permission: {error}"))?;
            let response = request
                .response()
                .map_err(|error| format!("Startup permission request failed: {error}"))?;
            Ok(response.auto_start())
        });
        let _ = sender.send(result);
    });

    let mut on_complete = Some(on_complete);
    glib::timeout_add_local(Duration::from_millis(100), move || {
        match receiver.try_recv() {
            Ok(result) => {
                if let Some(on_complete) = on_complete.take() {
                    on_complete(result);
                }
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => {
                if let Some(on_complete) = on_complete.take() {
                    on_complete(Err(
                        "Startup permission request stopped unexpectedly.".to_string()
                    ));
                }
                glib::ControlFlow::Break
            }
        }
    });
}

fn desktop_exec_quote(path: &Path) -> String {
    let escaped = path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[derive(Debug, Clone)]
enum TrayCommand {
    Show,
    QuickPick,
    CheckUpdate,
    Settings,
    PauseFor(Option<u64>),
    PauseUntilTomorrow,
    Resume,
    Quit,
}

#[derive(Debug)]
struct YanklogTray {
    sender: mpsc::Sender<TrayCommand>,
    update_available: Option<String>,
    pause_state: Arc<PauseState>,
}

impl ksni::Tray for YanklogTray {
    fn id(&self) -> String {
        "yanklog".to_string()
    }

    fn title(&self) -> String {
        if self.pause_state.is_paused() {
            "yanklog · monitoring paused".to_string()
        } else {
            "yanklog".to_string()
        }
    }

    fn icon_name(&self) -> String {
        String::new()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        let paused = self.pause_state.is_paused();
        vec![yanklog_tray_icon(32, paused), yanklog_tray_icon(64, paused)]
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        let mut items: Vec<ksni::MenuItem<Self>> = vec![
            StandardItem {
                label: "Show yanklog".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(TrayCommand::Show);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Quick Pick".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(TrayCommand::QuickPick);
                }),
                ..Default::default()
            }
            .into(),
        ];

        if self.pause_state.is_paused() {
            items.push(
                StandardItem {
                    label: "Resume Monitoring".into(),
                    activate: Box::new(|tray: &mut Self| {
                        let _ = tray.sender.send(TrayCommand::Resume);
                    }),
                    ..Default::default()
                }
                .into(),
            );
        } else {
            items.push(
                SubMenu {
                    label: "Pause Monitoring".into(),
                    submenu: vec![
                        StandardItem {
                            label: "15 minutes".into(),
                            activate: Box::new(|tray: &mut Self| {
                                let _ = tray.sender.send(TrayCommand::PauseFor(Some(15 * 60)));
                            }),
                            ..Default::default()
                        }
                        .into(),
                        StandardItem {
                            label: "1 hour".into(),
                            activate: Box::new(|tray: &mut Self| {
                                let _ = tray.sender.send(TrayCommand::PauseFor(Some(60 * 60)));
                            }),
                            ..Default::default()
                        }
                        .into(),
                        StandardItem {
                            label: "Until tomorrow".into(),
                            activate: Box::new(|tray: &mut Self| {
                                let _ = tray.sender.send(TrayCommand::PauseUntilTomorrow);
                            }),
                            ..Default::default()
                        }
                        .into(),
                        StandardItem {
                            label: "Indefinitely".into(),
                            activate: Box::new(|tray: &mut Self| {
                                let _ = tray.sender.send(TrayCommand::PauseFor(None));
                            }),
                            ..Default::default()
                        }
                        .into(),
                    ],
                    ..Default::default()
                }
                .into(),
            );
        }

        if let Some(version) = &self.update_available {
            items.push(
                StandardItem {
                    label: format!("Update available: yanklog {version}"),
                    activate: Box::new(|tray: &mut Self| {
                        let _ = tray.sender.send(TrayCommand::CheckUpdate);
                    }),
                    ..Default::default()
                }
                .into(),
            );
        }

        if !is_flatpak_build() {
            items.push(
                StandardItem {
                    label: "Check for update".into(),
                    activate: Box::new(|tray: &mut Self| {
                        let _ = tray.sender.send(TrayCommand::CheckUpdate);
                    }),
                    ..Default::default()
                }
                .into(),
            );
        }

        items.extend([
            StandardItem {
                label: "Settings".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(TrayCommand::Settings);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(TrayCommand::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]);
        items
    }
}

fn yanklog_tray_icon(size: i32, paused: bool) -> ksni::Icon {
    let size = size.max(16);
    let mut data = vec![0_u8; (size * size * 4) as usize];
    let scale = size as f32 / 64.0;

    fill_rounded_rect(
        &mut data,
        size,
        rect(22.0, 13.0, 36.0, 44.0, scale),
        if paused {
            (110, 110, 110, 100)
        } else {
            (102, 102, 241, 100)
        },
        radius(5.0, scale),
    );
    fill_rounded_rect(
        &mut data,
        size,
        rect(14.0, 8.0, 36.0, 44.0, scale),
        if paused {
            (130, 130, 130, 180)
        } else {
            (102, 102, 241, 180)
        },
        radius(5.0, scale),
    );
    fill_rounded_rect(
        &mut data,
        size,
        rect(6.0, 3.0, 36.0, 44.0, scale),
        (255, 255, 255, 255),
        radius(5.0, scale),
    );
    stroke_rounded_rect(
        &mut data,
        size,
        rect(6.0, 3.0, 36.0, 44.0, scale),
        if paused {
            (120, 120, 120, 255)
        } else {
            (102, 102, 241, 255)
        },
        radius(5.0, scale),
        line_width(3.0, scale),
    );
    fill_rounded_rect(
        &mut data,
        size,
        rect(12.0, 15.0, 24.0, 4.0, scale),
        if paused {
            (120, 120, 120, 255)
        } else {
            (102, 102, 241, 255)
        },
        radius(2.0, scale),
    );
    fill_rounded_rect(
        &mut data,
        size,
        rect(12.0, 24.0, 24.0, 4.0, scale),
        if paused {
            (170, 170, 170, 255)
        } else {
            (165, 180, 252, 255)
        },
        radius(2.0, scale),
    );
    fill_rounded_rect(
        &mut data,
        size,
        rect(12.0, 33.0, 17.0, 4.0, scale),
        if paused {
            (170, 170, 170, 255)
        } else {
            (165, 180, 252, 255)
        },
        radius(2.0, scale),
    );

    ksni::Icon {
        width: size,
        height: size,
        data,
    }
}

#[derive(Clone, Copy)]
struct IconRect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

fn rect(x: f32, y: f32, width: f32, height: f32, scale: f32) -> IconRect {
    IconRect {
        x: (x * scale).round() as i32,
        y: (y * scale).round() as i32,
        width: (width * scale).round().max(1.0) as i32,
        height: (height * scale).round().max(1.0) as i32,
    }
}

fn radius(value: f32, scale: f32) -> i32 {
    (value * scale).round().max(1.0) as i32
}

fn line_width(value: f32, scale: f32) -> i32 {
    (value * scale).round().max(1.0) as i32
}

fn fill_rounded_rect(
    data: &mut [u8],
    size: i32,
    rect: IconRect,
    color: (u8, u8, u8, u8),
    radius: i32,
) {
    for y in rect.y..(rect.y + rect.height) {
        for x in rect.x..(rect.x + rect.width) {
            if is_inside_rounded_rect(x, y, rect, radius) {
                blend_icon_pixel(data, size, x, y, color);
            }
        }
    }
}

fn stroke_rounded_rect(
    data: &mut [u8],
    size: i32,
    rect: IconRect,
    color: (u8, u8, u8, u8),
    radius: i32,
    width: i32,
) {
    let inner = IconRect {
        x: rect.x + width,
        y: rect.y + width,
        width: rect.width - (width * 2),
        height: rect.height - (width * 2),
    };
    for y in rect.y..(rect.y + rect.height) {
        for x in rect.x..(rect.x + rect.width) {
            if !is_inside_rounded_rect(x, y, rect, radius) {
                continue;
            }
            let inside_inner = inner.width > 0
                && inner.height > 0
                && is_inside_rounded_rect(x, y, inner, radius - width);
            if !inside_inner {
                blend_icon_pixel(data, size, x, y, color);
            }
        }
    }
}

fn is_inside_rounded_rect(x: i32, y: i32, rect: IconRect, radius: i32) -> bool {
    if x < rect.x || y < rect.y || x >= rect.x + rect.width || y >= rect.y + rect.height {
        return false;
    }
    if rect.width <= 0 || rect.height <= 0 {
        return false;
    }

    let radius = radius
        .max(0)
        .min((rect.width.saturating_sub(1)) / 2)
        .min((rect.height.saturating_sub(1)) / 2);
    if radius == 0 {
        return true;
    }
    let left = rect.x + radius;
    let right = rect.x + rect.width - radius - 1;
    let top = rect.y + radius;
    let bottom = rect.y + rect.height - radius - 1;

    let corner_x = x.clamp(left, right);
    let corner_y = y.clamp(top, bottom);
    let dx = x - corner_x;
    let dy = y - corner_y;
    dx * dx + dy * dy <= radius * radius
}

fn blend_icon_pixel(data: &mut [u8], size: i32, x: i32, y: i32, color: (u8, u8, u8, u8)) {
    if x < 0 || y < 0 || x >= size || y >= size {
        return;
    }
    let index = ((y * size + x) * 4) as usize;
    let (red, green, blue, alpha) = color;
    let source_alpha = alpha as f32 / 255.0;
    let dest_alpha = data[index] as f32 / 255.0;
    let out_alpha = source_alpha + dest_alpha * (1.0 - source_alpha);
    if out_alpha <= f32::EPSILON {
        return;
    }

    data[index] = (out_alpha * 255.0).round() as u8;
    data[index + 1] = ((red as f32 * source_alpha
        + data[index + 1] as f32 * dest_alpha * (1.0 - source_alpha))
        / out_alpha)
        .round() as u8;
    data[index + 2] = ((green as f32 * source_alpha
        + data[index + 2] as f32 * dest_alpha * (1.0 - source_alpha))
        / out_alpha)
        .round() as u8;
    data[index + 3] = ((blue as f32 * source_alpha
        + data[index + 3] as f32 * dest_alpha * (1.0 - source_alpha))
        / out_alpha)
        .round() as u8;
}

struct AppState {
    database: Arc<Mutex<Database>>,
    config: Arc<Mutex<Config>>,
    list: gtk::ListBox,
    search_entry: gtk::SearchEntry,
    status_label: gtk::Label,
    result_label: gtk::Label,
    previous_button: gtk::Button,
    next_button: gtk::Button,
    page_offset: Rc<Cell<usize>>,
    search_debounce: Rc<Cell<u64>>,
    last_deleted: Rc<RefCell<Option<yanklog_core::ClipboardEntry>>>,
    undo_button: gtk::Button,
}

fn refresh_entries(state: &AppState, query: Option<String>) {
    while let Some(child) = state.list.first_child() {
        state.list.remove(&child);
    }

    let query = query.unwrap_or_default();
    let query = query.trim();
    let (entries, total, offset) = match state.database.lock() {
        Ok(database) => {
            let total = if query.is_empty() {
                database.count_entries().unwrap_or_default()
            } else {
                database.count_search_history(query).unwrap_or_default()
            };
            let requested_offset = state.page_offset.get();
            let offset = if total == 0 {
                0
            } else {
                requested_offset.min(((total - 1) / HISTORY_PAGE_SIZE) * HISTORY_PAGE_SIZE)
            };
            state.page_offset.set(offset);
            let entries = if query.is_empty() {
                database
                    .get_history_page(HISTORY_PAGE_SIZE, offset)
                    .unwrap_or_default()
            } else {
                database
                    .search_history_page(query, HISTORY_PAGE_SIZE, offset)
                    .unwrap_or_default()
            };
            (entries, total, offset)
        }
        Err(_) => (Vec::new(), 0, 0),
    };

    state.previous_button.set_sensitive(offset > 0);
    state
        .next_button
        .set_sensitive(offset.saturating_add(entries.len()) < total);
    let result_text = if total == 0 {
        "0 items".to_string()
    } else {
        format!(
            "{}–{} of {}",
            offset + 1,
            offset.saturating_add(entries.len()),
            total
        )
    };
    state.result_label.set_text(&result_text);

    if entries.is_empty() {
        let empty_message = if query.is_empty() {
            "No clipboard history yet"
        } else {
            "No matching clipboard items"
        };
        let label = gtk::Label::new(Some(empty_message));
        label.add_css_class("empty-state");
        label.set_margin_top(120);
        label.set_margin_bottom(120);
        state.list.append(&label);
        return;
    }

    for entry in entries {
        let row = gtk::ListBoxRow::new();
        row.add_css_class("history-row");
        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        row_box.set_margin_top(14);
        row_box.set_margin_bottom(14);
        row_box.set_margin_start(16);
        row_box.set_margin_end(16);

        let text_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        text_box.set_hexpand(true);
        let preview_text = state
            .config
            .lock()
            .map(|config| {
                let limit = config.max_preview_length.min(180);
                truncate_preview(&entry.content, limit)
            })
            .unwrap_or_else(|_| entry.content.replace('\n', " "));
        let preview = gtk::Label::new(Some(&preview_text));
        preview.set_xalign(0.0);
        preview.set_wrap(true);
        preview.set_max_width_chars(90);
        preview.set_lines(2);
        preview.set_ellipsize(gtk::pango::EllipsizeMode::End);
        preview.add_css_class("row-preview");
        let meta = gtk::Label::new(Some(&format!(
            "{}{}",
            yanklog_core::format_timestamp(&entry.timestamp),
            if entry.is_favorite { " · pinned" } else { "" }
        )));
        meta.set_xalign(0.0);
        meta.add_css_class("row-meta");

        text_box.append(&preview);
        text_box.append(&meta);

        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        actions.add_css_class("entry-actions");
        actions.set_valign(gtk::Align::Start);
        let pin_button = gtk::Button::with_label(if entry.is_favorite { "Unpin" } else { "Pin" });
        pin_button.add_css_class("entry-action");
        let detail_button = gtk::Button::with_label("See more");
        detail_button.add_css_class("entry-action");
        let delete_button = gtk::Button::with_label("Delete");
        delete_button.add_css_class("entry-action");
        delete_button.add_css_class("destructive-action");

        {
            let action_state = state.clone_for_callbacks();
            let entry_id = entry.id;
            let was_favorite = entry.is_favorite;
            pin_button.connect_clicked(move |_| {
                if let Ok(database) = action_state.database.lock() {
                    let _ = database.toggle_favorite(entry_id);
                }
                refresh_with_current_query(&action_state);
                show_status(
                    &action_state.status_label,
                    if was_favorite {
                        "Entry unpinned"
                    } else {
                        "Entry pinned"
                    },
                );
            });
        }

        {
            let action_state = state.clone_for_callbacks();
            let entry = entry.clone();
            detail_button.connect_clicked(move |_| {
                show_entry_detail_window(&entry, action_state.clone_for_callbacks())
            });
        }

        {
            let action_state = state.clone_for_callbacks();
            let deleted_entry = entry.clone();
            delete_button.connect_clicked(move |_| {
                delete_entry_with_undo(&action_state, deleted_entry.clone());
            });
        }

        {
            let content = entry.content.clone();
            let status_label = state.status_label.clone();
            let gesture = gtk::GestureClick::new();
            gesture.connect_released(move |_, _, _, _| {
                if copy_to_clipboard(&content).is_ok() {
                    show_status(&status_label, "Copied to clipboard");
                }
            });
            text_box.add_controller(gesture);
        }

        actions.append(&pin_button);
        actions.append(&detail_button);
        actions.append(&delete_button);
        row_box.append(&text_box);
        row_box.append(&actions);
        row.set_child(Some(&row_box));
        state.list.append(&row);
    }
}

impl AppState {
    fn clone_for_callbacks(&self) -> Self {
        Self {
            database: Arc::clone(&self.database),
            config: Arc::clone(&self.config),
            list: self.list.clone(),
            search_entry: self.search_entry.clone(),
            status_label: self.status_label.clone(),
            result_label: self.result_label.clone(),
            previous_button: self.previous_button.clone(),
            next_button: self.next_button.clone(),
            page_offset: Rc::clone(&self.page_offset),
            search_debounce: Rc::clone(&self.search_debounce),
            last_deleted: Rc::clone(&self.last_deleted),
            undo_button: self.undo_button.clone(),
        }
    }
}

fn delete_entry_with_undo(state: &AppState, entry: yanklog_core::ClipboardEntry) {
    let deleted = state
        .database
        .lock()
        .map(|database| database.delete_entry(entry.id).is_ok())
        .unwrap_or(false);
    if !deleted {
        show_status(&state.status_label, "Could not delete entry");
        return;
    }

    state.last_deleted.replace(Some(entry));
    state.undo_button.set_visible(true);
    refresh_with_current_query(state);
    show_status(&state.status_label, "Entry deleted");
}

fn restore_last_deleted(state: &AppState) {
    let Some(entry) = state.last_deleted.borrow_mut().take() else {
        state.undo_button.set_visible(false);
        return;
    };
    let restored = state
        .database
        .lock()
        .map(|database| database.restore_entry(&entry).is_ok())
        .unwrap_or(false);
    if restored {
        state.undo_button.set_visible(false);
        state.page_offset.set(0);
        refresh_with_current_query(state);
        show_status(&state.status_label, "Entry restored");
    } else {
        state.last_deleted.replace(Some(entry));
        show_status(&state.status_label, "Could not restore entry");
    }
}

fn show_clear_history_confirmation(parent: &gtk::ApplicationWindow, state: AppState) {
    let (total, unpinned) = state
        .database
        .lock()
        .map(|database| {
            (
                database.count_entries().unwrap_or_default(),
                database.count_unpinned_entries().unwrap_or_default(),
            )
        })
        .unwrap_or_default();
    if total == 0 {
        show_status(&state.status_label, "Clipboard history is already empty");
        return;
    }

    let dialog = gtk::Window::builder()
        .title("Clear clipboard history?")
        .modal(true)
        .transient_for(parent)
        .default_width(440)
        .build();
    let title = gtk::Label::new(Some("Clear clipboard history?"));
    title.set_xalign(0.0);
    title.add_css_class("privacy-title");
    let message = gtk::Label::new(Some(&format!(
        "This affects {total} saved item{}. You can keep pinned items, but clearing cannot be undone.",
        if total == 1 { "" } else { "s" }
    )));
    message.set_xalign(0.0);
    message.set_wrap(true);
    message.add_css_class("settings-note");

    let cancel_button = gtk::Button::with_label("Cancel");
    let keep_pinned_button = gtk::Button::with_label(&format!("Clear {unpinned} unpinned"));
    keep_pinned_button.set_sensitive(unpinned > 0);
    let clear_all_button = gtk::Button::with_label(&format!("Clear all {total}"));
    clear_all_button.add_css_class("destructive-action");

    {
        let dialog = dialog.clone();
        cancel_button.connect_clicked(move |_| dialog.close());
    }
    {
        let dialog = dialog.clone();
        let state = state.clone_for_callbacks();
        keep_pinned_button.connect_clicked(move |_| {
            let deleted = state
                .database
                .lock()
                .ok()
                .and_then(|database| database.clear_unpinned_history().ok())
                .unwrap_or_default();
            state.page_offset.set(0);
            refresh_with_current_query(&state);
            show_status(
                &state.status_label,
                &format!("Cleared {deleted} unpinned items"),
            );
            dialog.close();
        });
    }
    {
        let dialog = dialog.clone();
        let state = state.clone_for_callbacks();
        clear_all_button.connect_clicked(move |_| {
            if let Ok(database) = state.database.lock() {
                let _ = database.clear_history();
            }
            state.page_offset.set(0);
            refresh_with_current_query(&state);
            show_status(&state.status_label, "Clipboard history cleared");
            dialog.close();
        });
    }

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::End);
    actions.append(&cancel_button);
    actions.append(&keep_pinned_button);
    actions.append(&clear_all_button);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 16);
    content.set_margin_top(20);
    content.set_margin_bottom(20);
    content.set_margin_start(20);
    content.set_margin_end(20);
    content.append(&title);
    content.append(&message);
    content.append(&actions);
    dialog.set_child(Some(&content));
    dialog.present();
}

fn show_status(label: &gtk::Label, message: &str) {
    label.set_text(message);
    let label = label.clone();
    let message = message.to_string();
    glib::timeout_add_seconds_local(2, move || {
        if label.text().as_str() == message {
            label.set_text("");
        }
        glib::ControlFlow::Break
    });
}

fn refresh_with_current_query(state: &AppState) {
    let query = state.search_entry.text().to_string();
    refresh_entries(state, if query.is_empty() { None } else { Some(query) });
}

fn show_entry_detail_window(entry: &yanklog_core::ClipboardEntry, state: AppState) {
    let window = gtk::Window::builder()
        .title("Clipboard entry")
        .default_width(620)
        .default_height(520)
        .decorated(true)
        .build();

    let title = gtk::Label::new(Some("Clipboard entry"));
    title.set_xalign(0.0);
    title.add_css_class("app-title");

    let meta = gtk::Label::new(Some(&format!(
        "{}{}",
        yanklog_core::format_timestamp(&entry.timestamp),
        if entry.is_favorite { " · pinned" } else { "" }
    )));
    meta.set_xalign(0.0);
    meta.add_css_class("muted");

    let text = gtk::TextView::new();
    text.set_editable(false);
    text.set_monospace(true);
    text.set_wrap_mode(gtk::WrapMode::WordChar);
    text.buffer().set_text(&entry.content);

    let scroller = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .child(&text)
        .build();
    scroller.add_css_class("history-list");

    let copy_button = gtk::Button::with_label("Copy");
    let pin_button = gtk::Button::with_label(if entry.is_favorite { "Unpin" } else { "Pin" });
    let delete_button = gtk::Button::with_label("Delete");
    delete_button.add_css_class("destructive-action");
    let close_button = gtk::Button::with_label("Close");
    let detail_status = gtk::Label::new(None);
    detail_status.set_xalign(0.0);
    detail_status.add_css_class("status-toast");

    {
        let content = entry.content.clone();
        let detail_status = detail_status.clone();
        copy_button.connect_clicked(move |_| {
            let _ = yanklog_core::copy_to_clipboard(&content);
            show_status(&detail_status, "Copied");
        });
    }

    {
        let state = state.clone_for_callbacks();
        let entry_id = entry.id;
        let was_favorite = entry.is_favorite;
        let window = window.clone();
        pin_button.connect_clicked(move |_| {
            if let Ok(database) = state.database.lock() {
                let _ = database.toggle_favorite(entry_id);
            }
            refresh_with_current_query(&state);
            show_status(
                &state.status_label,
                if was_favorite {
                    "Entry unpinned"
                } else {
                    "Entry pinned"
                },
            );
            window.close();
        });
    }

    {
        let state = state.clone_for_callbacks();
        let deleted_entry = entry.clone();
        let window = window.clone();
        delete_button.connect_clicked(move |_| {
            delete_entry_with_undo(&state, deleted_entry.clone());
            window.close();
        });
    }

    {
        let window = window.clone();
        close_button.connect_clicked(move |_| window.close());
    }

    let action_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    action_row.append(&copy_button);
    action_row.append(&pin_button);
    action_row.append(&delete_button);
    action_row.append(&spacer);
    action_row.append(&close_button);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 14);
    content.add_css_class("app-root");
    content.set_margin_top(16);
    content.set_margin_bottom(24);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.append(&title);
    content.append(&meta);
    content.append(&scroller);
    content.append(&detail_status);
    content.append(&action_row);

    window.set_child(Some(&content));
    window.present();
}

fn truncate_preview(text: &str, max_chars: usize) -> String {
    let text = text.replace('\n', " ").replace('\r', "");
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return text;
    }
    let truncated: String = chars.into_iter().take(max_chars).collect();
    format!("{truncated}...")
}

fn start_clipboard_monitor(
    database: Arc<Mutex<Database>>,
    monitor: ClipboardMonitor,
    config: Config,
    pause_state: Arc<PauseState>,
) {
    thread::spawn(move || loop {
        if pause_state.is_paused() {
            thread::sleep(Duration::from_millis(500));
            continue;
        }

        if let Some(content) = monitor.check_for_changes() {
            if !config.should_ignore_clipboard(&content) {
                if let Ok(database) = database.lock() {
                    let _ = database.insert_entry(&content, "text");
                    let _ = database.prune_old_entries(config.max_history_size);
                    let _ = database.clear_older_than_days(config.retention.max_age_days);
                }
            }
        }
        thread::sleep(Duration::from_millis(config.poll_interval_ms));
    });
}

fn run_quick_picker() {
    let application = adw::Application::builder()
        .application_id(picker_app_id())
        .build();
    application.connect_activate(|app| {
        show_quick_picker_window(app);
    });
    application.run_with_args(&["yanklog-picker"]);
}

fn show_quick_picker_window(app: &adw::Application) {
    install_css();
    let profile = profile();
    let config = Rc::new(Config::load(&profile).unwrap_or_default());
    apply_theme(&config);
    let database = match open_database_with_secure_key(&profile) {
        Ok(database) => Rc::new(database),
        Err(err) => {
            show_error_dialog(None, &format!("Failed to open yanklog database: {err}"));
            return;
        }
    };
    let entries = Rc::new(std::cell::RefCell::new(Vec::new()));

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Quick Pick")
        .default_width(QUICK_PICKER_WIDTH)
        .default_height(QUICK_PICKER_HEIGHT)
        .decorated(false)
        .build();
    window.add_css_class("quick-picker-window");
    window.set_resizable(false);
    window.set_size_request(QUICK_PICKER_WIDTH, QUICK_PICKER_HEIGHT);
    window.set_opacity(config.keybindings.quick_pick_opacity.clamp(0.4, 1.0));
    let search_entry = gtk::SearchEntry::builder()
        .placeholder_text("Search clipboard history")
        .build();
    search_entry.set_hexpand(true);
    let shortcut_help_button = gtk::Button::with_label("?");
    shortcut_help_button.set_tooltip_text(Some("Quick picker shortcut setup"));
    let list = gtk::ListBox::new();
    list.add_css_class("history-list");
    list.set_selection_mode(gtk::SelectionMode::Single);
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .child(&list)
        .build();

    {
        let app = app.clone();
        shortcut_help_button.connect_clicked(move |_| show_quick_picker_shortcut_help(&app));
    }

    populate_quick_picker_rows(&list, &database, &config, &entries, "");
    set_quick_picker_selection(&list, &scroller, 0);

    {
        let list = list.clone();
        let scroller = scroller.clone();
        let database = Rc::clone(&database);
        let config = Rc::clone(&config);
        let entries = Rc::clone(&entries);
        let debounce = Rc::new(Cell::new(0_u64));
        search_entry.connect_search_changed(move |entry| {
            let generation = debounce.get().wrapping_add(1);
            debounce.set(generation);
            let query = entry.text().to_string();
            let list = list.clone();
            let scroller = scroller.clone();
            let database = Rc::clone(&database);
            let config = Rc::clone(&config);
            let entries = Rc::clone(&entries);
            let delayed_debounce = Rc::clone(&debounce);
            glib::timeout_add_local_once(Duration::from_millis(160), move || {
                if delayed_debounce.get() != generation {
                    return;
                }
                populate_quick_picker_rows(&list, &database, &config, &entries, &query);
                set_quick_picker_selection(&list, &scroller, 0);
            });
        });
    }

    {
        let entries = Rc::clone(&entries);
        let window = window.clone();
        list.connect_row_activated(move |_, row| {
            if let Some(entry) = entries.borrow().get(row.index() as usize) {
                let _ = yanklog_core::copy_to_clipboard(&entry.content);
            }
            window.close();
        });
    }

    let key_controller = gtk::EventControllerKey::new();
    key_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let list = list.clone();
        let scroller = scroller.clone();
        let entries = Rc::clone(&entries);
        let window = window.clone();
        let database = Rc::clone(&database);
        let config = Rc::clone(&config);
        let search_entry = search_entry.clone();
        key_controller.connect_key_pressed(move |_, key, _, modifiers| {
            if modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK) {
                if let Some(index) = key
                    .to_unicode()
                    .and_then(|character| character.to_digit(10))
                    .and_then(|digit| digit.checked_sub(1))
                    .map(|index| index as usize)
                {
                    if index < 9 {
                        if let Some(entry) = entries.borrow().get(index) {
                            let _ = copy_to_clipboard(&entry.content);
                            window.close();
                        }
                        return glib::Propagation::Stop;
                    }
                }

                if key == gtk::gdk::Key::p {
                    if let Some(row) = list.selected_row() {
                        let selected = row.index();
                        if let Some(entry) = entries.borrow().get(selected as usize) {
                            let _ = database.toggle_favorite(entry.id);
                        }
                        populate_quick_picker_rows(
                            &list,
                            &database,
                            &config,
                            &entries,
                            &search_entry.text(),
                        );
                        set_quick_picker_selection(&list, &scroller, selected);
                    }
                    return glib::Propagation::Stop;
                }

                if key == gtk::gdk::Key::Delete {
                    if let Some(row) = list.selected_row() {
                        let selected = row.index();
                        if let Some(entry) = entries.borrow().get(selected as usize) {
                            let _ = database.delete_entry(entry.id);
                        }
                        populate_quick_picker_rows(
                            &list,
                            &database,
                            &config,
                            &entries,
                            &search_entry.text(),
                        );
                        let last = entries.borrow().len().saturating_sub(1) as i32;
                        set_quick_picker_selection(&list, &scroller, selected.min(last));
                    }
                    return glib::Propagation::Stop;
                }
            }

            match key {
                gtk::gdk::Key::Escape => {
                    window.close();
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Down => {
                    move_quick_picker_selection(&list, &scroller, entries.borrow().len(), 1);
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Up => {
                    move_quick_picker_selection(&list, &scroller, entries.borrow().len(), -1);
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::j => {
                    move_quick_picker_selection(&list, &scroller, entries.borrow().len(), 1);
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::k => {
                    move_quick_picker_selection(&list, &scroller, entries.borrow().len(), -1);
                    glib::Propagation::Stop
                }
                gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter => {
                    if let Some(row) = list.selected_row() {
                        if let Some(entry) = entries.borrow().get(row.index() as usize) {
                            let _ = yanklog_core::copy_to_clipboard(&entry.content);
                        }
                    }
                    window.close();
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
    }
    window.add_controller(key_controller);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.add_css_class("app-root");
    content.add_css_class("quick-picker-root");
    content.set_margin_top(16);
    content.set_margin_bottom(16);
    content.set_margin_start(16);
    content.set_margin_end(16);
    let search_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    search_row.append(&search_entry);
    search_row.append(&shortcut_help_button);
    let hint = gtk::Label::new(Some(
        "↑/↓ Navigate   Enter Copy   Ctrl+1–9 Copy   Ctrl+P Pin   Ctrl+Delete Remove   Esc Close",
    ));
    hint.set_xalign(0.0);
    hint.add_css_class("quick-picker-hint");
    content.append(&search_row);
    content.append(&hint);
    content.append(&scroller);
    window.set_child(Some(&content));
    let has_been_active = Rc::new(std::cell::Cell::new(false));
    {
        let has_been_active = Rc::clone(&has_been_active);
        window.connect_is_active_notify(move |window| {
            if window.is_active() {
                has_been_active.set(true);
                return;
            }

            if has_been_active.get() {
                window.close();
            }
        });
    }
    glib::timeout_add_seconds_local_once(1, {
        let window = window.clone();
        let has_been_active = Rc::clone(&has_been_active);
        move || {
            if !window.is_active() && !has_been_active.get() {
                window.close();
            }
        }
    });
    window.present();
    glib::idle_add_local_once(move || {
        search_entry.grab_focus();
    });
}

fn move_quick_picker_selection(
    list: &gtk::ListBox,
    scroller: &gtk::ScrolledWindow,
    item_count: usize,
    delta: i32,
) {
    if item_count == 0 {
        return;
    }
    let current = list
        .selected_row()
        .map(|row| row.index())
        .unwrap_or(0)
        .clamp(0, item_count.saturating_sub(1) as i32);
    let next = (current + delta).clamp(0, item_count.saturating_sub(1) as i32);
    set_quick_picker_selection(list, scroller, next);
}

fn set_quick_picker_selection(list: &gtk::ListBox, scroller: &gtk::ScrolledWindow, index: i32) {
    let mut row_index = 0;
    while let Some(row) = list.row_at_index(row_index) {
        row.remove_css_class("quick-selected");
        row_index += 1;
    }

    if let Some(row) = list.row_at_index(index) {
        list.select_row(Some(&row));
        row.add_css_class("quick-selected");
        scroll_quick_picker_row_into_view(scroller, &row);
    } else {
        list.unselect_all();
    }
}

fn scroll_quick_picker_row_into_view(scroller: &gtk::ScrolledWindow, row: &gtk::ListBoxRow) {
    let adjustment = scroller.vadjustment();
    let allocation = row.allocation();
    let row_top = f64::from(allocation.y());
    let row_bottom = row_top + f64::from(allocation.height());
    let visible_top = adjustment.value();
    let visible_bottom = visible_top + adjustment.page_size();

    if row_top < visible_top {
        adjustment.set_value(row_top.max(adjustment.lower()));
    } else if row_bottom > visible_bottom {
        let max_value = (adjustment.upper() - adjustment.page_size()).max(adjustment.lower());
        adjustment.set_value((row_bottom - adjustment.page_size()).min(max_value));
    }
}

fn populate_quick_picker_rows(
    list: &gtk::ListBox,
    database: &Database,
    config: &Config,
    entries: &Rc<std::cell::RefCell<Vec<yanklog_core::ClipboardEntry>>>,
    query: &str,
) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    let limit = Some(config.keybindings.quick_pick_items.max(1));
    let next_entries = if query.trim().is_empty() {
        database.get_history(limit).unwrap_or_default()
    } else {
        database.search_history(query, limit).unwrap_or_default()
    };
    entries.replace(next_entries.clone());

    if next_entries.is_empty() {
        let empty = gtk::Label::new(Some(if query.trim().is_empty() {
            "No clipboard history yet"
        } else {
            "No matching clipboard items"
        }));
        empty.add_css_class("empty-state");
        empty.set_margin_top(80);
        empty.set_margin_bottom(80);
        list.append(&empty);
        return;
    }

    for (index, entry) in next_entries.into_iter().enumerate() {
        let row = gtk::ListBoxRow::new();
        row.add_css_class("history-row");
        let row_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        row_box.set_margin_top(10);
        row_box.set_margin_bottom(10);
        row_box.set_margin_start(14);
        row_box.set_margin_end(14);
        let label = gtk::Label::new(Some(&truncate_preview(&entry.content, 140)));
        label.set_xalign(0.0);
        label.set_wrap(true);
        label.set_lines(2);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.add_css_class("row-preview");
        let meta = gtk::Label::new(Some(&format!(
            "{}{}{}",
            yanklog_core::format_timestamp(&entry.timestamp),
            if entry.is_favorite { " · Pinned" } else { "" },
            if index < 9 {
                format!(" · Ctrl+{}", index + 1)
            } else {
                String::new()
            }
        )));
        meta.set_xalign(0.0);
        meta.add_css_class("row-meta");
        row_box.append(&label);
        row_box.append(&meta);
        row.set_child(Some(&row_box));
        list.append(&row);
    }
}

fn show_preferences_window(app: &adw::Application, profile: &Profile, state: Rc<AppState>) {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Preferences")
        .default_width(560)
        .default_height(640)
        .decorated(true)
        .resizable(true)
        .build();
    window.set_size_request(480, 420);
    let shared_config = Arc::clone(&state.config);
    let config = shared_config
        .lock()
        .map(|config| config.clone())
        .unwrap_or_else(|_| Config::load(profile).unwrap_or_default());

    let title = gtk::Label::new(Some("Settings"));
    title.set_xalign(0.0);
    title.add_css_class("app-title");

    let profile_label = gtk::Label::new(Some(profile.display_name()));
    profile_label.set_xalign(0.0);
    profile_label.add_css_class("muted");

    let title_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    title_box.append(&title);
    title_box.append(&profile_label);

    let general_panel = gtk::Box::new(gtk::Orientation::Vertical, 12);
    general_panel.add_css_class("settings-panel");
    let quick_pick_panel = gtk::Box::new(gtk::Orientation::Vertical, 12);
    quick_pick_panel.add_css_class("settings-panel");
    let privacy_panel = gtk::Box::new(gtk::Orientation::Vertical, 12);
    privacy_panel.add_css_class("settings-panel");
    let history_limit = spin_row(
        "History limit",
        1.0,
        100_000.0,
        config.max_history_size as f64,
    );
    let preview_length = spin_row(
        "Preview length",
        40.0,
        2_000.0,
        config.max_preview_length as f64,
    );
    let quick_pick_items = spin_row(
        "Quick pick items",
        1.0,
        50.0,
        config.keybindings.quick_pick_items as f64,
    );
    let quick_pick_opacity = float_spin_row(
        "Quick pick opacity",
        0.4,
        1.0,
        config.keybindings.quick_pick_opacity.clamp(0.4, 1.0),
        0.05,
    );
    let theme_mode = theme_row(config.theme);
    let retention_days = spin_row(
        "Retention days",
        0.0,
        3650.0,
        config.retention.max_age_days as f64,
    );
    let min_text_length = spin_row(
        "Minimum text length",
        0.0,
        10_000.0,
        config.privacy.min_text_length as f64,
    );
    let max_text_length = spin_row(
        "Maximum text length",
        0.0,
        1_000_000.0,
        config.privacy.max_text_length as f64,
    );
    let ignored_patterns = entry_row("Ignored patterns", &config.ignored_patterns_text());
    let ignore_secret_like = gtk::CheckButton::with_label("Ignore secret-like values");
    ignore_secret_like.set_active(config.privacy.ignore_secret_like);
    ignore_secret_like.add_css_class("row-preview");
    let ignore_one_time_codes = gtk::CheckButton::with_label("Ignore one-time codes");
    ignore_one_time_codes.set_active(config.privacy.ignore_one_time_codes);
    ignore_one_time_codes.add_css_class("row-preview");
    let launch_at_startup = gtk::CheckButton::with_label("Run at startup");
    launch_at_startup.set_active(config.launch_at_startup);
    launch_at_startup.add_css_class("row-preview");
    let shortcut_help_button = gtk::Button::with_label("Shortcut setup");
    shortcut_help_button.set_halign(gtk::Align::Start);

    general_panel.append(&settings_note("Changes save automatically."));
    general_panel.append(&history_limit.0);
    general_panel.append(&preview_length.0);
    general_panel.append(&theme_mode.0);
    general_panel.append(&launch_at_startup);

    quick_pick_panel.append(&info_row(
        "Quick picker command",
        &quick_picker_command_text(),
    ));
    quick_pick_panel.append(&settings_note(
        "Bind this command in your desktop keyboard settings. Linux shortcut registration is handled by the desktop environment, so GNOME, KDE, Xfce, and other distros expose it in different places.",
    ));
    quick_pick_panel.append(&shortcut_help_button);
    quick_pick_panel.append(&quick_pick_items.0);
    quick_pick_panel.append(&quick_pick_opacity.0);

    privacy_panel.append(&retention_days.0);
    privacy_panel.append(&min_text_length.0);
    privacy_panel.append(&max_text_length.0);
    privacy_panel.append(&ignored_patterns.0);
    privacy_panel.append(&ignore_secret_like);
    privacy_panel.append(&ignore_one_time_codes);

    let preferences_status = gtk::Label::new(None);
    preferences_status.set_xalign(0.0);
    preferences_status.add_css_class("status-toast");
    let storage = storage_panel(&window, profile, &preferences_status, Rc::clone(&state));
    let about_panel = gtk::Box::new(gtk::Orientation::Vertical, 12);
    about_panel.add_css_class("settings-panel");
    about_panel.append(&info_row("Version", APP_VERSION));
    about_panel.append(&info_row(
        "Distribution",
        if is_flatpak_build() {
            "Flatpak"
        } else {
            "Linux direct install / AppImage"
        },
    ));
    about_panel.append(&settings_note(
        "YankLog keeps clipboard content on this device and does not transmit your history.",
    ));

    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.set_vexpand(true);
    stack.add_titled(&general_panel, Some("general"), "General");
    stack.add_titled(&quick_pick_panel, Some("quick-pick"), "Quick Pick");
    stack.add_titled(&privacy_panel, Some("privacy"), "Privacy");
    stack.add_titled(&storage, Some("storage"), "Storage");
    stack.add_titled(&about_panel, Some("about"), "About");
    let switcher = gtk::StackSwitcher::new();
    switcher.set_stack(Some(&stack));
    switcher.set_halign(gtk::Align::Center);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
    content.add_css_class("app-root");
    content.set_margin_top(12);
    content.set_margin_bottom(24);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.append(&title_box);
    content.append(&switcher);
    content.append(&stack);
    content.append(&preferences_status);

    {
        let app = app.clone();
        shortcut_help_button.connect_clicked(move |_| show_quick_picker_shortcut_help(&app));
    }

    let controls = SettingsControls {
        history_limit: history_limit.1,
        preview_length: preview_length.1,
        quick_pick_items: quick_pick_items.1,
        quick_pick_opacity: quick_pick_opacity.1,
        theme_mode: theme_mode.1,
        retention_days: retention_days.1,
        min_text_length: min_text_length.1,
        max_text_length: max_text_length.1,
        ignored_patterns: ignored_patterns.1,
        ignore_secret_like,
        ignore_one_time_codes,
        launch_at_startup,
    };
    connect_auto_save_controls(&controls, profile, &shared_config, &preferences_status);

    let scroller = gtk::ScrolledWindow::new();
    scroller.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroller.set_hexpand(true);
    scroller.set_vexpand(true);
    scroller.set_child(Some(&content));

    window.set_child(Some(&scroller));
    window.present();
}

#[derive(Clone)]
struct SettingsControls {
    history_limit: gtk::SpinButton,
    preview_length: gtk::SpinButton,
    quick_pick_items: gtk::SpinButton,
    quick_pick_opacity: gtk::SpinButton,
    theme_mode: gtk::DropDown,
    retention_days: gtk::SpinButton,
    min_text_length: gtk::SpinButton,
    max_text_length: gtk::SpinButton,
    ignored_patterns: gtk::Entry,
    ignore_secret_like: gtk::CheckButton,
    ignore_one_time_codes: gtk::CheckButton,
    launch_at_startup: gtk::CheckButton,
}

fn connect_auto_save_controls(
    controls: &SettingsControls,
    profile: &Profile,
    shared_config: &Arc<Mutex<Config>>,
    status_label: &gtk::Label,
) {
    macro_rules! connect_save {
        ($widget:expr, $signal:ident) => {{
            let widget = $widget.clone();
            let controls = controls.clone();
            let profile = profile.clone();
            let shared_config = Arc::clone(shared_config);
            let status_label = status_label.clone();
            widget.$signal(move |_| {
                save_preferences(&controls, &profile, &shared_config, &status_label)
            });
        }};
    }

    connect_save!(controls.history_limit, connect_value_changed);
    connect_save!(controls.preview_length, connect_value_changed);
    connect_save!(controls.quick_pick_items, connect_value_changed);
    connect_save!(controls.quick_pick_opacity, connect_value_changed);
    connect_save!(controls.theme_mode, connect_selected_notify);
    connect_save!(controls.retention_days, connect_value_changed);
    connect_save!(controls.min_text_length, connect_value_changed);
    connect_save!(controls.max_text_length, connect_value_changed);
    connect_save!(controls.ignored_patterns, connect_changed);
    connect_save!(controls.ignore_secret_like, connect_toggled);
    connect_save!(controls.ignore_one_time_codes, connect_toggled);
    connect_save!(controls.launch_at_startup, connect_toggled);
}

fn save_preferences(
    controls: &SettingsControls,
    profile: &Profile,
    shared_config: &Arc<Mutex<Config>>,
    status_label: &gtk::Label,
) {
    let previous = Config::load(profile).unwrap_or_default();
    let mut next = previous.clone();
    next.max_history_size = controls.history_limit.value().round() as usize;
    next.max_preview_length = controls.preview_length.value().round() as usize;
    next.keybindings.quick_pick_items = controls.quick_pick_items.value().round() as usize;
    next.keybindings.quick_pick_opacity = controls.quick_pick_opacity.value().clamp(0.4, 1.0);
    next.theme = theme_from_index(controls.theme_mode.selected());
    next.retention.max_age_days = controls.retention_days.value().round() as u32;
    next.privacy.min_text_length = controls.min_text_length.value().round() as usize;
    next.privacy.max_text_length = controls.max_text_length.value().round() as usize;
    next.privacy.ignore_secret_like = controls.ignore_secret_like.is_active();
    next.privacy.ignore_one_time_codes = controls.ignore_one_time_codes.is_active();
    next.launch_at_startup = controls.launch_at_startup.is_active();
    next.set_ignored_patterns_text(&controls.ignored_patterns.text());

    if is_flatpak_build() && previous.launch_at_startup != next.launch_at_startup {
        let launch_at_startup = controls.launch_at_startup.clone();
        let profile = profile.clone();
        let shared_config = Arc::clone(shared_config);
        let status_label = status_label.clone();
        launch_at_startup.set_sensitive(false);
        request_flatpak_launch_at_startup(next.launch_at_startup, move |result| {
            launch_at_startup.set_sensitive(true);
            match result {
                Ok(enabled) if enabled == next.launch_at_startup => {
                    save_preferences_result(next, &profile, &shared_config, &status_label);
                }
                Ok(_) => {
                    launch_at_startup.set_active(previous.launch_at_startup);
                    show_status(
                        &status_label,
                        "Start at login was not enabled by the desktop portal.",
                    );
                }
                Err(error) => {
                    launch_at_startup.set_active(previous.launch_at_startup);
                    show_status(&status_label, &error);
                }
            }
        });
        return;
    }

    if !is_flatpak_build() && previous.launch_at_startup != next.launch_at_startup {
        if let Err(error) = set_launch_at_startup(profile, next.launch_at_startup) {
            show_status(status_label, &error);
            return;
        }
    }

    save_preferences_result(next, profile, shared_config, status_label);
}

fn save_preferences_result(
    next: Config,
    profile: &Profile,
    shared_config: &Arc<Mutex<Config>>,
    status_label: &gtk::Label,
) {
    match next.save_linux_app(profile) {
        Ok(()) => {
            if let Ok(mut config) = shared_config.lock() {
                *config = next.clone();
            }
            apply_theme(&next);
            show_status(status_label, "Saved automatically");
        }
        Err(error) => show_status(status_label, &format!("Could not save settings: {error}")),
    }
}

fn settings_row_label(label: &str) -> gtk::Label {
    let label_widget = gtk::Label::new(Some(label));
    label_widget.set_xalign(0.0);
    label_widget.set_width_chars(18);
    label_widget.add_css_class("muted");
    label_widget
}

fn spin_row(label: &str, min: f64, max: f64, value: f64) -> (gtk::Box, gtk::SpinButton) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let input = gtk::SpinButton::with_range(min, max, 1.0);
    input.set_value(value);
    input.set_hexpand(true);
    row.append(&settings_row_label(label));
    row.append(&input);
    (row, input)
}

fn float_spin_row(
    label: &str,
    min: f64,
    max: f64,
    value: f64,
    step: f64,
) -> (gtk::Box, gtk::SpinButton) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let input = gtk::SpinButton::with_range(min, max, step);
    input.set_digits(2);
    input.set_value(value);
    input.set_hexpand(true);
    row.append(&settings_row_label(label));
    row.append(&input);
    (row, input)
}

fn theme_row(value: ThemePreference) -> (gtk::Box, gtk::DropDown) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let input = gtk::DropDown::from_strings(&["System", "Light", "Dark"]);
    input.set_selected(theme_index(value));
    input.set_hexpand(true);
    row.append(&settings_row_label("Appearance"));
    row.append(&input);
    (row, input)
}

fn theme_index(value: ThemePreference) -> u32 {
    match value {
        ThemePreference::System => 0,
        ThemePreference::Light => 1,
        ThemePreference::Dark => 2,
    }
}

fn theme_from_index(index: u32) -> ThemePreference {
    match index {
        1 => ThemePreference::Light,
        2 => ThemePreference::Dark,
        _ => ThemePreference::System,
    }
}

fn entry_row(label: &str, value: &str) -> (gtk::Box, gtk::Entry) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let input = gtk::Entry::new();
    input.set_text(value);
    input.set_hexpand(true);
    row.append(&settings_row_label(label));
    row.append(&input);
    (row, input)
}

fn info_row(label: &str, value: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.set_valign(gtk::Align::Start);
    let row_label = settings_row_label(label);
    row_label.set_valign(gtk::Align::Start);
    let value_label = gtk::Label::new(Some(value));
    value_label.set_xalign(0.0);
    value_label.set_wrap(true);
    value_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    value_label.set_max_width_chars(64);
    value_label.set_selectable(true);
    value_label.set_hexpand(true);
    value_label.add_css_class("row-preview");
    row.append(&row_label);
    row.append(&value_label);
    row
}

fn storage_panel(
    parent: &gtk::ApplicationWindow,
    profile: &Profile,
    status_label: &gtk::Label,
    state: Rc<AppState>,
) -> gtk::Box {
    let database_path = profile.data_dir().join("history.db");
    let database_path_text = database_path.to_string_lossy().to_string();

    let panel = gtk::Box::new(gtk::Orientation::Vertical, 10);
    panel.add_css_class("settings-panel");

    let title = gtk::Label::new(Some("Storage"));
    title.set_xalign(0.0);
    title.add_css_class("privacy-title");

    let key_storage = if profile.data_dir().join("secret.key").exists() {
        "Protected local file (Secret Service unavailable)"
    } else {
        "Linux Secret Service"
    };
    let note = settings_note(
        "Clipboard history is stored locally. Backups use password-based XChaCha20-Poly1305 encryption.",
    );
    let path_row = info_row("Database", &database_path_text);
    let key_row = info_row("Database key", key_storage);

    let copy_button = gtk::Button::with_label("Copy Path");
    let reveal_button = gtk::Button::with_label("Reveal in Files");
    let export_button = gtk::Button::with_label("Export Backup");
    let import_button = gtk::Button::with_label("Restore Backup");
    let action_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    action_row.append(&copy_button);
    action_row.append(&reveal_button);
    action_row.append(&export_button);
    action_row.append(&import_button);

    {
        let database_path_text = database_path_text.clone();
        let status_label = status_label.clone();
        copy_button.connect_clicked(move |_| {
            let _ = copy_to_clipboard(&database_path_text);
            show_status(&status_label, "Database path copied");
        });
    }

    {
        let status_label = status_label.clone();
        reveal_button.connect_clicked(move |_| {
            let reveal_path = database_path
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_else(|| database_path.clone());
            let _ = std::process::Command::new("xdg-open")
                .arg(reveal_path)
                .spawn();
            show_status(&status_label, "Opening database location");
        });
    }

    {
        let parent = parent.clone();
        let profile = profile.clone();
        let state = Rc::clone(&state);
        let status_label = status_label.clone();
        export_button.connect_clicked(move |_| {
            let chooser = gtk::FileChooserNative::new(
                Some("Export encrypted YankLog backup"),
                Some(&parent),
                gtk::FileChooserAction::Save,
                Some("Export"),
                Some("Cancel"),
            );
            let _ = chooser.set_current_name("yanklog-backup.yanklog");
            let parent = parent.clone();
            let profile = profile.clone();
            let state = Rc::clone(&state);
            let status_label = status_label.clone();
            chooser.run_async(move |chooser, response| {
                if response == gtk::ResponseType::Accept {
                    if let Some(path) = chooser.file().and_then(|file| file.path()) {
                        show_export_backup_dialog(
                            &parent,
                            &profile,
                            Rc::clone(&state),
                            path,
                            &status_label,
                        );
                    }
                }
                chooser.destroy();
            });
        });
    }

    {
        let parent = parent.clone();
        let profile = profile.clone();
        let state = Rc::clone(&state);
        let status_label = status_label.clone();
        import_button.connect_clicked(move |_| {
            let chooser = gtk::FileChooserNative::new(
                Some("Restore encrypted YankLog backup"),
                Some(&parent),
                gtk::FileChooserAction::Open,
                Some("Restore"),
                Some("Cancel"),
            );
            let filter = gtk::FileFilter::new();
            filter.set_name(Some("YankLog backups"));
            filter.add_pattern("*.yanklog");
            chooser.set_filter(&filter);
            let parent = parent.clone();
            let profile = profile.clone();
            let state = Rc::clone(&state);
            let status_label = status_label.clone();
            chooser.run_async(move |chooser, response| {
                if response == gtk::ResponseType::Accept {
                    if let Some(path) = chooser.file().and_then(|file| file.path()) {
                        show_import_backup_dialog(
                            &parent,
                            &profile,
                            Rc::clone(&state),
                            path,
                            &status_label,
                        );
                    }
                }
                chooser.destroy();
            });
        });
    }

    panel.append(&title);
    panel.append(&note);
    panel.append(&path_row);
    panel.append(&key_row);
    panel.append(&action_row);
    panel
}

fn show_export_backup_dialog(
    parent: &gtk::ApplicationWindow,
    profile: &Profile,
    state: Rc<AppState>,
    path: PathBuf,
    status_label: &gtk::Label,
) {
    let dialog = gtk::Window::builder()
        .title("Encrypt backup")
        .modal(true)
        .transient_for(parent)
        .default_width(460)
        .build();
    let title = gtk::Label::new(Some("Protect your backup"));
    title.set_xalign(0.0);
    title.add_css_class("privacy-title");
    let scope = gtk::DropDown::from_strings(&["All history", "Pinned only"]);
    let password = gtk::PasswordEntry::new();
    password.set_placeholder_text(Some("Password (at least 8 characters)"));
    password.set_show_peek_icon(true);
    let confirmation = gtk::PasswordEntry::new();
    confirmation.set_placeholder_text(Some("Confirm password"));
    confirmation.set_show_peek_icon(true);
    let include_settings = gtk::CheckButton::with_label("Include settings");
    include_settings.set_active(true);
    let dialog_status = gtk::Label::new(None);
    dialog_status.set_xalign(0.0);
    dialog_status.add_css_class("status-toast");
    let cancel = gtk::Button::with_label("Cancel");
    let export = gtk::Button::with_label("Export Encrypted Backup");
    export.add_css_class("suggested-action");

    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| dialog.close());
    }
    {
        let dialog = dialog.clone();
        let profile = profile.clone();
        let status_label = status_label.clone();
        let scope = scope.clone();
        let password = password.clone();
        let confirmation = confirmation.clone();
        let include_settings = include_settings.clone();
        let dialog_status = dialog_status.clone();
        export.connect_clicked(move |_| {
            let password_text = password.text().to_string();
            if password_text != confirmation.text() {
                show_status(&dialog_status, "Passwords do not match");
                return;
            }
            let mut entries = state
                .database
                .lock()
                .ok()
                .and_then(|database| database.get_history(None).ok())
                .unwrap_or_default();
            if scope.selected() == 1 {
                entries.retain(|entry| entry.is_favorite);
            }
            if entries.is_empty() {
                show_status(&dialog_status, "There are no items in this backup scope");
                return;
            }
            let count = entries.len();
            let settings = include_settings
                .is_active()
                .then(|| Config::load(&profile).unwrap_or_default());
            match yanklog_core::backup::write_encrypted(&path, &password_text, entries, settings) {
                Ok(()) => {
                    show_status(&status_label, &format!("Exported {count} encrypted items"));
                    dialog.close();
                }
                Err(error) => show_status(&dialog_status, &error),
            }
        });
    }

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::End);
    actions.append(&cancel);
    actions.append(&export);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(20);
    content.set_margin_bottom(20);
    content.set_margin_start(20);
    content.set_margin_end(20);
    content.append(&title);
    content.append(&scope);
    content.append(&password);
    content.append(&confirmation);
    content.append(&include_settings);
    content.append(&dialog_status);
    content.append(&actions);
    dialog.set_child(Some(&content));
    dialog.present();
}

fn show_import_backup_dialog(
    parent: &gtk::ApplicationWindow,
    profile: &Profile,
    state: Rc<AppState>,
    path: PathBuf,
    status_label: &gtk::Label,
) {
    let dialog = gtk::Window::builder()
        .title("Restore backup")
        .modal(true)
        .transient_for(parent)
        .default_width(460)
        .build();
    let title = gtk::Label::new(Some("Restore encrypted backup"));
    title.set_xalign(0.0);
    title.add_css_class("privacy-title");
    let note = settings_note(
        "Existing matching items are updated; other history remains untouched. Pinned state and timestamps are preserved.",
    );
    let password = gtk::PasswordEntry::new();
    password.set_placeholder_text(Some("Backup password"));
    password.set_show_peek_icon(true);
    let include_settings = gtk::CheckButton::with_label("Restore included settings");
    include_settings.set_active(true);
    let dialog_status = gtk::Label::new(None);
    dialog_status.set_xalign(0.0);
    dialog_status.add_css_class("status-toast");
    let cancel = gtk::Button::with_label("Cancel");
    let restore = gtk::Button::with_label("Restore Backup");
    restore.add_css_class("suggested-action");

    {
        let dialog = dialog.clone();
        cancel.connect_clicked(move |_| dialog.close());
    }
    {
        let dialog = dialog.clone();
        let profile = profile.clone();
        let status_label = status_label.clone();
        let password = password.clone();
        let include_settings = include_settings.clone();
        let dialog_status = dialog_status.clone();
        restore.connect_clicked(move |_| {
            let backup = match yanklog_core::backup::read_encrypted(&path, &password.text()) {
                Ok(backup) => backup,
                Err(error) => {
                    show_status(&dialog_status, &error);
                    return;
                }
            };
            let count = backup.entries.len();
            let restored = state
                .database
                .lock()
                .map(|database| {
                    backup
                        .entries
                        .iter()
                        .all(|entry| database.restore_entry(entry).is_ok())
                })
                .unwrap_or(false);
            if !restored {
                show_status(&dialog_status, "Could not restore every backup item");
                return;
            }
            if include_settings.is_active() {
                if let Some(config) = backup.config {
                    if let Err(error) = config.save_linux_app(&profile) {
                        show_status(
                            &dialog_status,
                            &format!("Items restored, but settings failed: {error}"),
                        );
                        return;
                    }
                    if let Ok(mut shared) = state.config.lock() {
                        *shared = config.clone();
                    }
                    apply_theme(&config);
                }
            }
            state.page_offset.set(0);
            refresh_with_current_query(&state);
            show_status(&status_label, &format!("Restored {count} encrypted items"));
            dialog.close();
        });
    }

    let actions = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    actions.set_halign(gtk::Align::End);
    actions.append(&cancel);
    actions.append(&restore);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(20);
    content.set_margin_bottom(20);
    content.set_margin_start(20);
    content.set_margin_end(20);
    content.append(&title);
    content.append(&note);
    content.append(&password);
    content.append(&include_settings);
    content.append(&dialog_status);
    content.append(&actions);
    dialog.set_child(Some(&content));
    dialog.present();
}

fn settings_note(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_margin_start(0);
    label.add_css_class("settings-note");
    label
}

fn quick_picker_command_path() -> String {
    find_appimage_path()
        .or_else(find_current_exe_path)
        .or_else(find_yanklog_in_path)
        .or_else(find_known_yanklog_launcher)
        .unwrap_or_else(|| "yanklog".to_string())
}

fn quick_picker_command_text() -> String {
    if is_flatpak_build() {
        return format!("flatpak run {FLATPAK_APP_ID} --pick");
    }

    let command = format!(
        "{} --pick",
        shell_quote_for_display(&quick_picker_command_path())
    );

    if profile().dev {
        format!("env YANKLOG_DEV_MODE=1 YANKLOG_DISABLE_UPDATE_CHECK=1 {command}")
    } else {
        command
    }
}

fn find_yanklog_in_path() -> Option<String> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join("yanklog");
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    None
}

fn find_appimage_path() -> Option<String> {
    let path = std::env::var_os("APPIMAGE")?;
    let path = std::path::PathBuf::from(path);
    path.is_file().then(|| path.to_string_lossy().to_string())
}

fn find_current_exe_path() -> Option<String> {
    std::env::current_exe()
        .ok()
        .filter(|path| path.is_file())
        .map(|path| path.to_string_lossy().to_string())
}

fn find_known_yanklog_launcher() -> Option<String> {
    let mut candidates = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(std::path::PathBuf::from(home).join(".local/bin/yanklog"));
    }
    candidates.push(std::path::PathBuf::from("/usr/local/bin/yanklog"));
    candidates.push(std::path::PathBuf::from("/usr/bin/yanklog"));

    candidates
        .into_iter()
        .find(|path| path.is_file())
        .map(|path| path.to_string_lossy().to_string())
}

fn shell_quote_for_display(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-'))
    {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn show_quick_picker_shortcut_help(app: &adw::Application) {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Quick picker shortcut")
        .default_width(600)
        .default_height(520)
        .decorated(true)
        .build();

    let title = gtk::Label::new(Some("Quick picker shortcut"));
    title.set_xalign(0.0);
    title.add_css_class("app-title");

    let shortcut_command = quick_picker_command_text();
    let intro = settings_note(
        "YankLog exposes the quick picker as a command. Bind this exact command in your Linux desktop keyboard settings:",
    );
    let command = info_row("Command", &shortcut_command);
    let path_note = settings_note(
        "If a shortcut works in Terminal but not from the desktop, use the full path above. Desktop shortcuts often run with a smaller PATH than your shell.",
    );

    let instructions = [
        (
            "GNOME",
            "Settings > Keyboard > View and Customize Shortcuts > Custom Shortcuts > Add. Use the command above and choose your preferred shortcut.",
        ),
        (
            "KDE Plasma",
            "System Settings > Keyboard > Shortcuts > Add Command. Enter the command above, then assign the shortcut.",
        ),
        (
            "Xfce",
            "Settings > Keyboard > Application Shortcuts > Add. Enter the command above, then press the shortcut.",
        ),
        (
            "Cinnamon",
            "System Settings > Keyboard > Shortcuts > Custom Shortcuts > Add custom shortcut. Enter the command above and assign a binding.",
        ),
        (
            "LXQt",
            "Preferences > LXQt settings > Shortcut Keys > Add. Set the command above and choose the shortcut.",
        ),
    ];

    let panel = gtk::Box::new(gtk::Orientation::Vertical, 12);
    panel.add_css_class("settings-panel");
    panel.append(&intro);
    panel.append(&command);
    panel.append(&path_note);
    for (name, body) in instructions {
        panel.append(&instruction_section(name, body));
    }

    let status_label = gtk::Label::new(None);
    status_label.set_xalign(0.0);
    status_label.add_css_class("status-toast");

    let copy_button = gtk::Button::with_label("Copy Command");
    {
        let shortcut_command = shortcut_command.clone();
        let status_label = status_label.clone();
        copy_button.connect_clicked(move |_| {
            let _ = copy_to_clipboard(&shortcut_command);
            show_status(&status_label, "Shortcut command copied");
        });
    }

    let close_button = gtk::Button::with_label("Close");
    {
        let window = window.clone();
        close_button.connect_clicked(move |_| window.close());
    }

    let action_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    action_row.append(&copy_button);
    action_row.append(&spacer);
    action_row.append(&close_button);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 16);
    content.add_css_class("app-root");
    content.set_margin_top(16);
    content.set_margin_bottom(24);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.append(&title);
    content.append(&panel);
    content.append(&status_label);
    content.append(&action_row);

    window.set_child(Some(&content));
    window.present();
}

fn instruction_section(name: &str, body: &str) -> gtk::Box {
    let section = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let title = gtk::Label::new(Some(name));
    title.set_xalign(0.0);
    title.add_css_class("privacy-title");
    let copy = settings_note(body);
    section.append(&title);
    section.append(&copy);
    section
}

fn show_error_dialog(parent: Option<&gtk::Window>, message: &str) {
    let Some(parent) = parent else {
        eprintln!("yanklog: {message}");
        return;
    };

    let dialog = gtk::Window::builder()
        .title("yanklog")
        .modal(true)
        .transient_for(parent)
        .default_width(360)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_top(18);
    content.set_margin_bottom(18);
    content.set_margin_start(18);
    content.set_margin_end(18);

    let label = gtk::Label::new(Some(message));
    label.set_wrap(true);
    label.set_xalign(0.0);
    let ok_button = gtk::Button::with_label("OK");
    ok_button.add_css_class("suggested-action");

    {
        let dialog = dialog.clone();
        ok_button.connect_clicked(move |_| dialog.close());
    }

    content.append(&label);
    content.append(&ok_button);
    dialog.set_child(Some(&content));
    dialog.present();
}
