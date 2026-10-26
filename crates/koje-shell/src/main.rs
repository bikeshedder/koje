//! koje-shell: koje's UI in one GTK application (ADR 0042) with libadwaita
//! (ADR 0043). For now the bar, one per monitor; later the OSD, the window
//! overlays, the notifications and, in its own process, the settings app.

mod app_info;
mod backend;
mod bar;
mod bar_layout;
mod capture;
mod clock;
mod contexts_ui;
mod dash;
mod dialog_ui;
mod end_session;
mod hypr;
mod menu;
mod menu_ui;
mod model;
mod overview;
mod overview_ui;
mod quick;
mod quick_ui;
mod search;
mod share;
mod share_ui;
mod switcher;
mod switcher_ui;
mod system;

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use libadwaita as adw;
use tokio::sync::mpsc::UnboundedSender;
use tracing_subscriber::EnvFilter;

use crate::bar::Bar;
use crate::model::{Action, Shell, Update, reconcile};
use crate::overview_ui::Mode;

/// Also the shell's name on the session bus: `GApplication` keeps it to one
/// instance, and the systemd unit waits for it.
const APP_ID: &str = "io.koje.Shell";

/// The CLI the shell runs for changes (`koje limit`); the package's.
pub const KOJE: &str = "koje";

/// The bar's look: GNOME's top bar (ADR 0047) – black in both styles, white
/// bold text, panel buttons that light up on hover and while their menu is
/// open, the workspace indicator's dots. Scoped to the bar's own widgets:
/// the dropdowns keep libadwaita's light and dark. The accents come from
/// [`Shell::accent_css`].
const CSS: &str = "
window.koje-bar { background: #000000; min-height: 32px; }
.panel-button, .panel-button > button {
  color: #ffffff;
  font-weight: bold;
  background: transparent;
  box-shadow: none;
  border-radius: 9999px;
  min-height: 24px;
}
.panel-button { margin: 3px 0; padding: 0 12px; }
.panel-button > button { padding: 0 12px; margin: 0; }
button.panel-button:hover, .panel-button > button:hover, .workspace-indicator:hover {
  background: alpha(#ffffff, 0.17);
}
button.panel-button:active, .panel-button > button:checked, .panel-button > button:active {
  background: alpha(#ffffff, 0.25);
}
.workspace-indicator { padding: 0 10px; }
window.koje-bar button.workspace-dot {
  min-width: 7px; min-height: 7px; padding: 0; margin: 0;
  border-radius: 9999px; background: #ffffff; background-image: none;
  box-shadow: none; opacity: 0.5;
}
window.koje-bar button.workspace-dot:hover { opacity: 0.8; }
window.koje-bar button.workspace-dot.colored { background: var(--accent-bg-color); opacity: 0.8; }
window.koje-bar button.workspace-dot.colored.koje-host { background: #ffffff; opacity: 0.5; }
window.koje-bar button.workspace-dot.active { min-width: 32px; opacity: 1; }
button.workspace-name.active { background: alpha(#ffffff, 0.25); }
.capsule {
  background: var(--accent-bg-color);
  color: var(--accent-fg-color);
  border-radius: 9999px;
  padding: 2px 10px;
  font-weight: bold;
}
.koje-bar .koje-icon { color: var(--accent-color); }
.koje-icon.koje-host { color: #ffffff; }
.koje-graph { color: var(--accent-color); }
.koje-bar .warning { color: var(--warning-color); margin: 0 8px; }
window.koje-dialog { background: transparent; }
.dialog-card {
  background: var(--dialog-bg-color);
  color: var(--dialog-fg-color);
  border-radius: 15px;
  padding: 24px;
  margin: 24px;
  box-shadow: 0 2px 12px alpha(black, 0.3), 0 0 0 1px alpha(black, 0.05);
}
";

type Bars = Rc<RefCell<Vec<Bar>>>;

fn main() -> glib::ExitCode {
    // stderr: `share-picker`'s stdout is its reply to xdph.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    // `koje-shell overview` / `apps`: show or hide the running shell's
    // overview or app grid (Super, Super+A; ADR 0049); `switch
    // [--backwards] [--windows]`: Alt+Tab (M8.49); `share-picker`: xdph's
    // screen sharing picker, its own process (ADR 0050).
    match std::env::args().nth(1).as_deref() {
        Some("overview") => return remote_action("overview"),
        Some("apps") => return remote_action("app-grid"),
        Some("capture-probe") => {
            let dir = std::env::args().nth(2).map(std::path::PathBuf::from);
            return match capture::probe(dir.as_deref()) {
                Ok(()) => glib::ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("{e:#}");
                    glib::ExitCode::FAILURE
                }
            };
        }
        Some("share-picker") => {
            let args: Vec<String> = std::env::args().skip(2).collect();
            return share_ui::run(&args);
        }
        Some("switch") => {
            let args: Vec<String> = std::env::args().skip(2).collect();
            let back = args.iter().any(|a| a == "--backwards");
            let windows = args.iter().any(|a| a == "--windows");
            return remote_action(switch_action(back, windows));
        }
        _ => {}
    }
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_startup(start);
    // A second `koje-shell` activates this one: the bars are already there.
    app.connect_activate(|_| {});
    // GTK must not parse koje-shell's arguments.
    app.run_with_args::<&str>(&[])
}

/// Activate an action of the running shell; never becomes the shell itself.
fn remote_action(action: &str) -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::IS_LAUNCHER)
        .build();
    if let Err(e) = app.register(None::<&gio::Cancellable>) {
        tracing::error!("koje-shell is not running: {e}");
        return glib::ExitCode::FAILURE;
    }
    app.activate_action(action, None);
    // The call goes out with the next flush.
    if let Some(conn) = app.dbus_connection() {
        let _ = conn.flush_sync(None::<&gio::Cancellable>);
    }
    glib::ExitCode::SUCCESS
}

fn start(app: &adw::Application) {
    let (tx, rx) = async_channel::unbounded();
    let capture = capture::spawn(tx.clone());
    let (actions, actions_rx) = tokio::sync::mpsc::unbounded_channel();
    let actions_for_confirm = actions.clone();
    if let Err(e) = backend::spawn(tx, actions_rx) {
        tracing::error!("{e:#}");
        app.quit();
        return;
    }
    let Some(display) = gdk::Display::default() else {
        tracing::error!("no display – koje-shell needs a Wayland session");
        app.quit();
        return;
    };
    let style = |css: &str| {
        let provider = gtk4::CssProvider::new();
        provider.load_from_string(css);
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        provider
    };
    style(CSS);
    style(&swatch_css());
    style(quick_ui::CSS);
    style(overview_ui::CSS);
    style(switcher_ui::CSS);
    let shell = Rc::new(RefCell::new(Shell::default()));
    let accents_css = shell.borrow().accent_css();
    let accents = style(&accents_css);

    let bars: Bars = Rc::default();
    let monitors = display.monitors();
    sync_bars(app, &monitors, &bars, &shell, &actions);
    monitors.connect_items_changed({
        let (app, bars, shell) = (app.clone(), bars.clone(), shell.clone());
        move |monitors, _, _, _| sync_bars(&app, monitors, &bars, &shell, &actions)
    });

    follow_config(&shell, &bars);
    follow_favorites(&shell, &bars);
    overview_actions(app, &bars, &shell, &actions_for_confirm, &capture);
    follow_host_apps(&shell, &bars);
    switcher_actions(app, &bars, &shell, &actions_for_confirm);
    // GNOME's questions with a countdown: a display change, ending the session.
    let display_dialog = dialog_ui::CountdownDialog::new(app, &actions_for_confirm);
    let end_dialog = dialog_ui::CountdownDialog::new(app, &actions_for_confirm);
    end_session_actions(app, &end_dialog);

    glib::spawn_future_local(async move {
        let mut accents_css = accents_css;
        while let Ok(update) = rx.recv().await {
            if let Update::Thumbnail { address, thumb } = update {
                let texture = texture(&thumb);
                for bar in bars.borrow().iter() {
                    bar.overview().set_thumbnail(&address, &texture);
                }
                continue;
            }
            if let Update::DisplayChangePending(seconds) = update {
                display_dialog.ask(
                    dialog_ui::Question {
                        title: "Keep these display settings?".to_owned(),
                        text: Box::new(end_session::display_revert_text),
                        cancel_label: "Revert Settings",
                        on_cancel: Some(Action::RevertDisplayChange),
                        confirm_label: "Keep Changes",
                        destructive: false,
                        on_confirm: Action::KeepDisplayChange,
                        // kojed reverts by itself.
                        on_timeout: None,
                    },
                    seconds,
                );
                continue;
            }
            shell.borrow_mut().apply(update);
            let css = shell.borrow().accent_css();
            if css != accents_css {
                accents.load_from_string(&css);
                accents_css = css;
            }
            render(&bars, &shell);
        }
    });

    // Keep running without monitors (all unplugged): bars come back with them.
    std::mem::forget(app.hold());
}

/// The color buttons of "New context…": each in its palette color.
fn swatch_css() -> String {
    use std::fmt::Write as _;
    let mut css = String::from(
        ".swatch { min-width: 24px; min-height: 24px; padding: 0; }\n\
         .swatch-auto { min-height: 24px; padding: 0 10px; }\n\
         .swatch:checked, .swatch-auto:checked { outline: 2px solid var(--accent-bg-color); outline-offset: 2px; }\n",
    );
    for (label, hex) in menu::PALETTE {
        let _ = writeln!(
            css,
            ".swatch-{} {{ background: {hex}; }}",
            label.to_lowercase()
        );
    }
    css
}

/// `[bar]` of `config.toml` now and whenever the file changes (ADR 0047);
/// an invalid file keeps the last good settings.
fn follow_config(shell: &Rc<RefCell<Shell>>, bars: &Bars) {
    let Ok(paths) = koje_core::paths::Paths::from_env() else {
        return;
    };
    let path = paths.global_config();
    let load = {
        let (shell, bars, path) = (shell.clone(), bars.clone(), path.clone());
        move || match koje_core::config::GlobalConfig::load(&path) {
            Ok(cfg) => {
                shell.borrow_mut().apply(Update::Bar(cfg.bar));
                render(&bars, &shell);
            }
            Err(e) => tracing::warn!("{e}"),
        }
    };
    load();
    match gio::File::for_path(&path)
        .monitor_file(gio::FileMonitorFlags::NONE, None::<&gio::Cancellable>)
    {
        Ok(monitor) => {
            monitor.connect_changed(move |_, _, _, event| {
                if matches!(
                    event,
                    gio::FileMonitorEvent::ChangesDoneHint
                        | gio::FileMonitorEvent::Created
                        | gio::FileMonitorEvent::Deleted
                ) {
                    load();
                }
            });
            // Watches as long as the shell runs.
            std::mem::forget(monitor);
        }
        Err(e) => tracing::warn!("watching {}: {e}", path.display()),
    }
}

/// The host's programs for the search and the app grid, now and whenever
/// they change.
fn follow_host_apps(shell: &Rc<RefCell<Shell>>, bars: &Bars) {
    let load = {
        let (shell, bars) = (shell.clone(), bars.clone());
        move || {
            shell
                .borrow_mut()
                .apply(Update::HostApps(app_info::host_apps()));
            render(&bars, &shell);
        }
    };
    load();
    let monitor = gio::AppInfoMonitor::get();
    monitor.connect_changed(move |_| load());
    std::mem::forget(monitor);
}

/// GNOME's favorite apps for the dash, now and whenever they change.
fn follow_favorites(shell: &Rc<RefCell<Shell>>, bars: &Bars) {
    let Some(settings) = app_info::shell_settings() else {
        tracing::info!("no org.gnome.shell schema: the dash has no favorites");
        return;
    };
    let load = {
        let (shell, bars) = (shell.clone(), bars.clone());
        move |s: &gio::Settings| {
            let favorites = s
                .strv("favorite-apps")
                .iter()
                .map(ToString::to_string)
                .collect();
            shell.borrow_mut().apply(Update::Favorites(favorites));
            render(&bars, &shell);
        }
    };
    load(&settings);
    settings.connect_changed(Some("favorite-apps"), move |s, _| load(s));
    // Follows as long as the shell runs.
    std::mem::forget(settings);
}

/// The shell's actions `log-out`, `restart` and `power-off` on the session
/// bus (`GApplication` exports them): they ask first (M8.47). `koje session
/// end` and Super+Shift+E use them.
fn end_session_actions(app: &adw::Application, dialog: &Rc<dialog_ui::CountdownDialog>) {
    let user = glib::real_name().to_string_lossy().into_owned();
    for what in end_session::EndSession::ALL {
        let action = gio::SimpleAction::new(what.action_name(), None);
        let (dialog, user) = (dialog.clone(), user.clone());
        action.connect_activate(move |_, _| {
            let user = user.clone();
            dialog.ask(
                dialog_ui::Question {
                    title: what.title().to_owned(),
                    text: Box::new(move |left| what.body(left, &user)),
                    cancel_label: "Cancel",
                    on_cancel: None,
                    confirm_label: what.title(),
                    destructive: true,
                    on_confirm: Action::EndSession(what),
                    on_timeout: Some(Action::EndSession(what)),
                },
                end_session::COUNTDOWN,
            );
        });
        app.add_action(&action);
    }
}

fn render(bars: &Bars, shell: &Rc<RefCell<Shell>>) {
    let shell = shell.borrow();
    for bar in bars.borrow().iter() {
        let connector = bar.connector();
        bar.render(&shell.bar(connector.as_deref()));
        if bar.overview().is_visible() {
            bar.overview()
                .render(&shell.overview(connector.as_deref(), &app_info::desktop_id));
        }
    }
}

/// The actions `overview` (show or hide it on every monitor; Super),
/// `app-grid` (the same with the app grid; Super+A) and `hide-overview`
/// (after a click in it, Escape).
fn overview_actions(
    app: &adw::Application,
    bars: &Bars,
    shell: &Rc<RefCell<Shell>>,
    actions: &UnboundedSender<Action>,
    capture: &std::sync::mpsc::Sender<Vec<String>>,
) {
    for (name, mode) in [("overview", Mode::Windows), ("app-grid", Mode::Apps)] {
        let action = gio::SimpleAction::new(name, None);
        let (bars, shell, actions) = (bars.clone(), shell.clone(), actions.clone());
        let capture = capture.clone();
        action.connect_activate(move |_, _| {
            toggle_overview(&bars, &shell, &actions, mode);
            // Fresh pictures of the windows it shows.
            let windows: Vec<String> = bars
                .borrow()
                .iter()
                .filter(|b| b.overview().is_visible())
                .flat_map(|b| b.overview().windows())
                .collect();
            if !windows.is_empty() {
                let _ = capture.send(windows);
            }
        });
        app.add_action(&action);
    }
    let hide = gio::SimpleAction::new("hide-overview", None);
    hide.connect_activate({
        let bars = bars.clone();
        move |_, _| hide_overviews(&bars)
    });
    app.add_action(&hide);
}

/// A thumbnail as a GDK texture.
fn texture(thumb: &capture::Thumbnail) -> gdk::Texture {
    let bytes = glib::Bytes::from(thumb.data.as_slice());
    let stride = usize::try_from(thumb.width).unwrap_or(0) * 4;
    gdk::MemoryTexture::new(
        i32::try_from(thumb.width).unwrap_or(1),
        i32::try_from(thumb.height).unwrap_or(1),
        gdk::MemoryFormat::B8g8r8a8Premultiplied,
        &bytes,
        stride,
    )
    .upcast()
}

/// The switcher's action for Alt+Tab, Alt+Shift+Tab, Alt+grave and Alt+Shift+grave.
fn switch_action(backwards: bool, windows: bool) -> &'static str {
    match (windows, backwards) {
        (false, false) => "switch-next",
        (false, true) => "switch-prev",
        (true, false) => "switch-windows",
        (true, true) => "switch-windows-prev",
    }
}

/// Alt+Tab (M8.49): the switcher opens on the focused monitor.
fn switcher_actions(
    app: &adw::Application,
    bars: &Bars,
    shell: &Rc<RefCell<Shell>>,
    actions: &UnboundedSender<Action>,
) {
    let switcher = switcher_ui::Switcher::new(app, actions.clone(), shell.clone());
    for windows in [false, true] {
        for backwards in [false, true] {
            let action = gio::SimpleAction::new(switch_action(backwards, windows), None);
            let (bars, shell, switcher) = (bars.clone(), shell.clone(), switcher.clone());
            action.connect_activate(move |_, _| {
                let focused = shell.borrow().focused_monitor().map(str::to_owned);
                let monitor = bars
                    .borrow()
                    .iter()
                    .find(|b| b.connector() == focused)
                    .map(|b| b.monitor().clone());
                switcher.next(monitor.as_ref(), backwards, windows);
            });
            app.add_action(&action);
        }
    }
}

/// Open the overview in `mode`; open in that mode, close it; open in the
/// other, switch.
fn toggle_overview(
    bars: &Bars,
    shell: &Rc<RefCell<Shell>>,
    actions: &UnboundedSender<Action>,
    mode: Mode,
) {
    let s = shell.borrow();
    let focused = s.focused_monitor();
    let bars = bars.borrow();
    let here = |bar: &Bar| focused.is_none_or(|f| bar.connector().as_deref() == Some(f));
    let main = bars.iter().find(|b| here(b)).or_else(|| bars.first());
    if bars.iter().any(|b| b.overview().is_visible()) {
        if main.is_some_and(|b| b.overview().mode() != mode) {
            main.inspect(|b| b.overview().set_mode(mode));
        } else {
            for bar in bars.iter() {
                bar.overview().hide();
            }
        }
        return;
    }
    // The contexts' programs may have changed since: read them again.
    let _ = actions.send(Action::RefreshApps(s.running()));
    for bar in bars.iter() {
        let connector = bar.connector();
        bar.overview()
            .render(&s.overview(connector.as_deref(), &app_info::desktop_id));
        let is_main = main.is_some_and(|m| std::ptr::eq(m, bar));
        if is_main {
            bar.overview().set_mode(mode);
        }
        bar.overview().show(is_main);
    }
}

fn hide_overviews(bars: &Bars) {
    for bar in bars.borrow().iter() {
        bar.overview().hide();
    }
}

/// One bar per monitor: new ones for new monitors, none for gone ones.
fn sync_bars(
    app: &adw::Application,
    monitors: &gio::ListModel,
    bars: &Bars,
    shell: &Rc<RefCell<Shell>>,
    actions: &UnboundedSender<Action>,
) {
    let want: Vec<gdk::Monitor> = monitors
        .iter::<gdk::Monitor>()
        .filter_map(Result::ok)
        .collect();
    {
        let mut bars = bars.borrow_mut();
        let have: Vec<gdk::Monitor> = bars.iter().map(|b| b.monitor().clone()).collect();
        let (remove, add) = reconcile(&have, &want);
        for i in remove {
            bars.remove(i).close();
        }
        for monitor in add {
            bars.push(Bar::new(app, monitor, actions.clone(), shell.clone()));
        }
    }
    render(bars, shell);
}
