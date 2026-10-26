//! koje-shell: koje's UI in one GTK application (ADR 0042) with libadwaita
//! (ADR 0043). For now the bar, one per monitor; later the OSD, the window
//! overlays, the notifications and, in its own process, the settings app.

mod backend;
mod bar;
mod model;

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use libadwaita as adw;
use tracing_subscriber::EnvFilter;

use crate::bar::Bar;
use crate::model::{Shell, reconcile};

/// Also the shell's name on the session bus: `GApplication` keeps it to one
/// instance, and the systemd unit waits for it.
const APP_ID: &str = "io.koje.Shell";

const CSS: &str = "
.koje-bar { min-height: 32px; }
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
    if let Err(e) = backend::spawn(tx) {
        tracing::error!("{e:#}");
        app.quit();
        return;
    }
    let Some(display) = gdk::Display::default() else {
        tracing::error!("no display – koje-shell needs a Wayland session");
        app.quit();
        return;
    };
    let css = gtk4::CssProvider::new();
    css.load_from_string(CSS);
    gtk4::style_context_add_provider_for_display(
        &display,
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let shell = Rc::new(RefCell::new(Shell::default()));
    let bars: Bars = Rc::default();
    let monitors = display.monitors();
    sync_bars(app, &monitors, &bars, &shell);
    monitors.connect_items_changed({
        let (app, bars, shell) = (app.clone(), bars.clone(), shell.clone());
        move |monitors, _, _, _| sync_bars(&app, monitors, &bars, &shell)
    });

    glib::spawn_future_local(async move {
        while let Ok(update) = rx.recv().await {
            shell.borrow_mut().apply(update);
            let view = shell.borrow().bar();
            for bar in bars.borrow().iter() {
                bar.render(&view);
            }
        }
    });

    // Keep running without monitors (all unplugged): bars come back with them.
    std::mem::forget(app.hold());
}

/// One bar per monitor: new ones for new monitors, none for gone ones.
fn sync_bars(
    app: &adw::Application,
    monitors: &gio::ListModel,
    bars: &Bars,
    shell: &Rc<RefCell<Shell>>,
) {
    let want: Vec<gdk::Monitor> = monitors
        .iter::<gdk::Monitor>()
        .filter_map(Result::ok)
        .collect();
    let mut bars = bars.borrow_mut();
    let have: Vec<gdk::Monitor> = bars.iter().map(|b| b.monitor().clone()).collect();
    let (remove, add) = reconcile(&have, &want);
    for i in remove {
        bars.remove(i).close();
    }
    let view = shell.borrow().bar();
    for monitor in add {
        let bar = Bar::new(app, monitor);
        bar.render(&view);
        bars.push(bar);
    }
}
