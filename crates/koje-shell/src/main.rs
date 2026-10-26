//! koje-shell: koje's UI in one GTK application (ADR 0042) with libadwaita
//! (ADR 0043). For now the bar, one per monitor; later the OSD, the window
//! overlays, the notifications and, in its own process, the settings app.

mod backend;
mod bar;
mod bar_layout;
mod clock;
mod confirm_ui;
mod hypr;
mod menu;
mod menu_ui;
mod model;
mod overview_ui;
mod quick;
mod quick_ui;
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
window.display-confirm { background: transparent; }
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
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_startup(start);
    // A second `koje-shell` activates this one: the bars are already there.
    app.connect_activate(|_| {});
    // GTK must not parse koje-shell's arguments.
    app.run_with_args::<&str>(&[])
}

fn start(app: &adw::Application) {
    let (tx, rx) = async_channel::unbounded();
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
    let confirm = confirm_ui::DisplayConfirm::new(app, &actions_for_confirm);

    glib::spawn_future_local(async move {
        let mut accents_css = accents_css;
        while let Ok(update) = rx.recv().await {
            if let Update::DisplayChangePending(seconds) = update {
                confirm.show(seconds);
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

fn render(bars: &Bars, shell: &Rc<RefCell<Shell>>) {
    let shell = shell.borrow();
    for bar in bars.borrow().iter() {
        bar.render(&shell.bar(bar.connector().as_deref()));
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
            bars.push(Bar::new(app, monitor, actions.clone()));
        }
    }
    render(bars, shell);
}
