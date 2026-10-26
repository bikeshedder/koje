//! koje-shell: koje's UI in one GTK application (ADR 0042) with libadwaita
//! (ADR 0043). For now the bar, one per monitor; later the OSD, the window
//! overlays, the notifications and, in its own process, the settings app.

mod backend;
mod bar;
mod hypr;
mod menu;
mod menu_ui;
mod model;

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use libadwaita as adw;
use tokio::sync::mpsc::UnboundedSender;
use tracing_subscriber::EnvFilter;

use crate::bar::Bar;
use crate::model::{Action, Shell, reconcile};

/// Also the shell's name on the session bus: `GApplication` keeps it to one
/// instance, and the systemd unit waits for it.
const APP_ID: &str = "io.koje.Shell";

/// The CLI the shell runs for changes (`koje limit`); the package's.
pub const KOJE: &str = "koje";

/// The bar's look; the accents come from [`Shell::accent_css`]. Workspace
/// buttons are pills, the active one filled with its accent; the focused
/// task is underlined with its workspace's accent.
const CSS: &str = "
.koje-bar { min-height: 32px; }
.koje-bar button { min-height: 24px; padding: 0 10px; }
.koje-bar .workspaces, .koje-bar .tasks { margin: 4px 0; }
.koje-bar .workspaces button { border-radius: 9999px; }
.koje-bar .workspaces button.active {
  background: var(--accent-bg-color);
  color: var(--accent-fg-color);
}
.koje-bar .tasks button { border-radius: 6px; padding: 0 8px; }
.koje-bar .tasks button.focused { box-shadow: inset 0 -2px var(--accent-bg-color); }
.capsule {
  background: var(--accent-bg-color);
  color: var(--accent-fg-color);
  border-radius: 9999px;
  padding: 2px 10px;
  font-weight: bold;
}
.koje-bar .koje-icon { color: var(--accent-color); }
.koje-graph { color: var(--accent-color); }
.koje-bar .warning { color: var(--warning-color); }
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

    glib::spawn_future_local(async move {
        let mut accents_css = accents_css;
        while let Ok(update) = rx.recv().await {
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
