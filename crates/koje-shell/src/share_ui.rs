//! `koje-shell share-picker` (ADR 0050): xdph's screen sharing picker, a
//! short-lived process of its own. It reads the windows, screens and context
//! colors once, shows a card like GNOME's picker – Windows by context, Screens,
//! *Remember this selection* – and prints the reply ([`share::reply`]) on
//! stdout; nothing on Cancel. A failure exits non-zero: the wrapper then runs
//! xdph's own picker.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write as _;
use std::rc::Rc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell as _};
use koje_hyprland::Hyprland;
use libadwaita as adw;

use crate::backend::DaemonProxy;
use crate::model::{RunningContext, Update};
use crate::share::{self, Choice, Screen, ShareView};

const APP_ID: &str = "io.koje.SharePicker";
const TIMEOUT: Duration = Duration::from_secs(2);

const CSS: &str = "
.share-card { padding: 18px; }
.share-tile { padding: 6px; border-radius: 12px; }
.share-tile:checked {
  background: alpha(var(--accent-bg-color), 0.25);
  box-shadow: inset 0 0 0 3px var(--accent-bg-color);
}
.share-preview { background: alpha(currentColor, 0.08); border-radius: 8px; }
.share-dot { color: var(--accent-color); }
.share-tag {
  background: alpha(currentColor, 0.1);
  border-radius: 9999px;
  padding: 0 8px;
  font-size: smaller;
}
";

/// What the picker needs to know, read before it shows.
struct Loaded {
    view: ShareView,
    contexts: Vec<RunningContext>,
}

/// Run the picker; `args` are xdph's (`--allow-token`).
pub fn run(args: &[String]) -> glib::ExitCode {
    let allow_token = args.iter().any(|a| a == "--allow-token");
    let loaded = match load(allow_token) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("{e:#}");
            return glib::ExitCode::FAILURE;
        }
    };
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let answer: Rc<RefCell<Option<String>>> = Rc::default();
    let failed = Rc::new(std::cell::Cell::new(false));
    let loaded = RefCell::new(Some(loaded));
    let picker: RefCell<Option<Rc<Picker>>> = RefCell::default();
    app.connect_activate({
        let (answer, failed) = (answer.clone(), failed.clone());
        move |app| {
            let Some(loaded) = loaded.borrow_mut().take() else {
                return;
            };
            if gdk::Display::default().is_none() {
                tracing::error!("no display – the picker needs the Wayland session");
                failed.set(true);
                return;
            }
            // Kept until the process ends: the widgets' handlers only hold
            // it weakly.
            picker.replace(Some(Picker::show(app, &loaded, answer.clone())));
        }
    });
    let code = app.run_with_args::<&str>(&[]);
    if failed.get() || code != glib::ExitCode::SUCCESS {
        return glib::ExitCode::FAILURE;
    }
    if let Some(line) = answer.borrow().as_deref() {
        let mut out = std::io::stdout();
        if out
            .write_all(line.as_bytes())
            .and_then(|()| out.flush())
            .is_err()
        {
            return glib::ExitCode::FAILURE;
        }
    }
    glib::ExitCode::SUCCESS
}

#[derive(serde::Deserialize)]
struct Monitor {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    focused: bool,
}

/// Hyprland's windows and screens (required) and kojed's contexts (their
/// colors; without kojed the groups stay grey).
fn load(allow_token: bool) -> Result<Loaded> {
    let list = share::parse_list(&std::env::var("XDPH_WINDOW_SHARING_LIST").unwrap_or_default());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting the picker's runtime")?;
    runtime.block_on(async {
        let hypr = Hyprland::from_env();
        let desktop = tokio::time::timeout(TIMEOUT, crate::hypr::desktop(&hypr))
            .await
            .context("Hyprland did not answer")??;
        let monitors = tokio::time::timeout(TIMEOUT, hypr.request("j/monitors"))
            .await
            .context("Hyprland did not answer")??;
        let monitors: Vec<Monitor> =
            serde_json::from_str(&monitors).context("Hyprland's monitors")?;
        let screens: Vec<Screen> = monitors
            .into_iter()
            .map(|m| Screen {
                name: m.name,
                description: m.description,
                focused: m.focused,
            })
            .collect();
        let contexts = match tokio::time::timeout(TIMEOUT, running_contexts()).await {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => {
                tracing::info!("no context colors: {e:#}");
                Vec::new()
            }
            Err(_) => {
                tracing::info!("no context colors: kojed did not answer");
                Vec::new()
            }
        };
        let view = share::view(&list, &desktop.windows, &contexts, &screens, allow_token);
        Ok(Loaded { view, contexts })
    })
}

async fn running_contexts() -> Result<Vec<RunningContext>> {
    let conn = zbus::Connection::session().await?;
    let daemon = DaemonProxy::new(&conn).await?;
    Ok(daemon
        .running_contexts()
        .await?
        .into_iter()
        .map(RunningContext::from)
        .collect())
}

struct Picker {
    window: gtk::Window,
    share: gtk::Button,
    remember: gtk::CheckButton,
    chosen: RefCell<Option<Choice>>,
    answer: Rc<RefCell<Option<String>>>,
    pictures: RefCell<HashMap<String, (gtk::Picture, gtk::Image)>>,
}

impl Picker {
    fn show(
        app: &adw::Application,
        loaded: &Loaded,
        answer: Rc<RefCell<Option<String>>>,
    ) -> Rc<Self> {
        if let Some(display) = gdk::Display::default() {
            for css in [crate::CSS, CSS, &crate::model::accent_css(&loaded.contexts)] {
                let provider = gtk::CssProvider::new();
                provider.load_from_string(css);
                gtk::style_context_add_provider_for_display(
                    &display,
                    &provider,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
            }
        }
        let window = gtk::Window::builder().application(app).build();
        window.add_css_class("koje-dialog");
        window.init_layer_shell();
        window.set_layer(Layer::Overlay);
        window.set_namespace(Some("koje-share-picker"));
        window.set_keyboard_mode(KeyboardMode::Exclusive);

        let share = gtk::Button::builder()
            .label("Share")
            .css_classes(["pill", "suggested-action"])
            .sensitive(false)
            .build();
        let remember = gtk::CheckButton::builder()
            .label("Remember this selection")
            .active(loaded.view.remember)
            .build();
        let me = Rc::new(Self {
            window,
            share,
            remember,
            chosen: RefCell::new(None),
            answer,
            pictures: RefCell::default(),
        });
        me.build(&loaded.view);
        me.thumbnails(&loaded.view);
        me.window.present();
        me
    }

    fn build(self: &Rc<Self>, view: &ShareView) {
        let title = gtk::Label::builder()
            .label("Share Screen")
            .css_classes(["title-2"])
            .build();
        let subtitle = gtk::Label::builder()
            .label("Choose a window or a screen to share")
            .css_classes(["dim-label"])
            .build();
        let stack = adw::ViewStack::new();
        // One radio group for every tile, windows and screens.
        let group: Rc<RefCell<Option<gtk::ToggleButton>>> = Rc::default();
        let windows = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .build();
        for g in &view.groups {
            windows.append(&self.group(g, &group));
        }
        if view.groups.is_empty() {
            windows.append(
                &adw::StatusPage::builder()
                    .icon_name("focus-windows-symbolic")
                    .title("No Windows")
                    .build(),
            );
        }
        let screens = flow();
        for s in &view.screens {
            let button = self.tile(
                &group,
                Choice::Screen(s.name.clone()),
                "koje-host",
                &preview_icon("video-display-symbolic"),
                &s.name,
                &s.description,
                false,
            );
            screens.insert(&button, -1);
        }
        let windows_page = stack.add_titled(&scrolled(&windows), Some("windows"), "Windows");
        windows_page.set_icon_name(Some("focus-windows-symbolic"));
        let screens_page = stack.add_titled(&scrolled(&screens), Some("screens"), "Screens");
        screens_page.set_icon_name(Some("video-display-symbolic"));
        if view.groups.is_empty() {
            stack.set_visible_child_name("screens");
        }
        let switcher = adw::ViewSwitcher::builder()
            .stack(&stack)
            .policy(adw::ViewSwitcherPolicy::Wide)
            .halign(gtk::Align::Center)
            .build();

        let cancel = gtk::Button::builder()
            .label("Cancel")
            .css_classes(["pill"])
            .build();
        let buttons = gtk::Box::builder().spacing(12).build();
        self.remember.set_hexpand(true);
        buttons.append(&self.remember);
        buttons.append(&cancel);
        buttons.append(&self.share);

        let card = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .width_request(760)
            .css_classes(["dialog-card", "share-card"])
            .build();
        card.append(&title);
        card.append(&subtitle);
        card.append(&switcher);
        card.append(&stack);
        card.append(&buttons);
        self.window.set_child(Some(&card));
        self.window.set_default_widget(Some(&self.share));

        let me = Rc::downgrade(self);
        cancel.connect_clicked(move |_| {
            if let Some(me) = me.upgrade() {
                me.close();
            }
        });
        let me = Rc::downgrade(self);
        self.share.connect_clicked(move |_| {
            if let Some(me) = me.upgrade() {
                me.finish();
            }
        });
        self.keys();
    }

    /// Before the focused widget: Enter on a tile shares it (a toggle
    /// button would only toggle), Escape cancels.
    fn keys(self: &Rc<Self>) {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let me = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(me) = me.upgrade() else {
                return glib::Propagation::Proceed;
            };
            match key {
                gdk::Key::Escape => me.close(),
                gdk::Key::Return | gdk::Key::KP_Enter => {
                    let Some(focused) = gtk::prelude::GtkWindowExt::focus(&me.window)
                        .and_then(|f| f.ancestor(gtk::ToggleButton::static_type()))
                        .and_downcast::<gtk::ToggleButton>()
                        .filter(|b| b.has_css_class("share-tile"))
                    else {
                        return glib::Propagation::Proceed;
                    };
                    focused.set_active(true);
                    me.finish();
                }
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        self.window.add_controller(keys);
    }

    /// A context's windows (or the host's) under its name in its color.
    fn group(
        self: &Rc<Self>,
        g: &share::Group,
        radio: &Rc<RefCell<Option<gtk::ToggleButton>>>,
    ) -> gtk::Box {
        let accent = g.accent.css_class();
        let header = gtk::Box::builder().spacing(8).build();
        header.append(
            &gtk::Label::builder()
                .label("●")
                .css_classes(["share-dot", accent.as_str()])
                .build(),
        );
        header.append(
            &gtk::Label::builder()
                .label(g.context.as_deref().unwrap_or("Host"))
                .css_classes(["heading"])
                .build(),
        );
        let tiles = flow();
        for w in &g.windows {
            let picture = gtk::Picture::builder()
                .can_shrink(true)
                .content_fit(gtk::ContentFit::Contain)
                .build();
            let icon = gtk::Image::from_gicon(&crate::app_info::icon(&w.app));
            icon.set_pixel_size(48);
            let layers = gtk::Overlay::builder().child(&picture).build();
            layers.add_overlay(&icon);
            if !w.address.is_empty() {
                self.pictures
                    .borrow_mut()
                    .insert(w.address.clone(), (picture, icon));
            }
            let button = self.tile(
                radio,
                Choice::Window(w.handle),
                &accent,
                layers.upcast_ref(),
                &w.title,
                &crate::app_info::name(&w.app),
                w.stashed,
            );
            tiles.insert(&button, -1);
        }
        let section = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .build();
        section.append(&header);
        section.append(&tiles);
        section
    }

    /// One choice: its preview, a title and a second line; a click selects
    /// it, a double click shares it.
    #[allow(clippy::too_many_arguments)]
    fn tile(
        self: &Rc<Self>,
        radio: &Rc<RefCell<Option<gtk::ToggleButton>>>,
        choice: Choice,
        accent: &str,
        preview: &gtk::Widget,
        title: &str,
        detail: &str,
        stashed: bool,
    ) -> gtk::ToggleButton {
        let frame = gtk::Box::builder()
            .css_classes(["share-preview"])
            .width_request(200)
            .height_request(125)
            .build();
        preview.set_hexpand(true);
        frame.append(preview);
        let title = gtk::Label::builder()
            .label(title)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .width_chars(1)
            .max_width_chars(24)
            .build();
        let line = gtk::Box::builder()
            .spacing(6)
            .halign(gtk::Align::Center)
            .build();
        line.append(
            &gtk::Label::builder()
                .label(detail)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .css_classes(["dim-label", "caption"])
                .build(),
        );
        if stashed {
            line.append(
                &gtk::Label::builder()
                    .label("stashed")
                    .css_classes(["share-tag"])
                    .tooltip_text("Running, but not in the context's active layout")
                    .build(),
            );
        }
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();
        content.append(&frame);
        content.append(&title);
        content.append(&line);
        let button = gtk::ToggleButton::builder()
            .child(&content)
            .css_classes(["flat", "share-tile", accent])
            .build();
        {
            let mut first = radio.borrow_mut();
            match first.as_ref() {
                Some(f) => button.set_group(Some(f)),
                None => *first = Some(button.clone()),
            }
        }
        let me = Rc::downgrade(self);
        button.connect_toggled(move |b| {
            if b.is_active()
                && let Some(me) = me.upgrade()
            {
                me.chosen.replace(Some(choice.clone()));
                me.share.set_sensitive(true);
            }
        });
        let twice = gtk::GestureClick::new();
        let me = Rc::downgrade(self);
        twice.connect_pressed(move |_, presses, _, _| {
            if presses == 2
                && let Some(me) = me.upgrade()
            {
                me.finish();
            }
        });
        button.add_controller(twice);
        button
    }

    /// The windows' pictures, from the overview's capture (M8.67).
    fn thumbnails(self: &Rc<Self>, view: &ShareView) {
        let addresses: Vec<String> = view
            .groups
            .iter()
            .flat_map(|g| &g.windows)
            .filter(|w| !w.address.is_empty())
            .map(|w| w.address.clone())
            .collect();
        if addresses.is_empty() {
            return;
        }
        let (tx, rx) = async_channel::unbounded();
        let requests = crate::capture::spawn(tx);
        let _ = requests.send(addresses);
        let me = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            // Keep the thread's request side alive while pictures arrive.
            let _requests = requests;
            while let Ok(update) = rx.recv().await {
                let Some(me) = me.upgrade() else {
                    return;
                };
                if let Update::Thumbnail { address, thumb } = update
                    && let Some((picture, icon)) = me.pictures.borrow().get(&address)
                {
                    picture.set_paintable(Some(&crate::texture(&thumb)));
                    icon.set_pixel_size(32);
                    icon.set_valign(gtk::Align::End);
                    icon.set_margin_bottom(6);
                }
            }
        });
    }

    fn finish(&self) {
        if let Some(choice) = self.chosen.borrow().as_ref() {
            self.answer
                .replace(Some(share::reply(choice, self.remember.is_active())));
            self.close();
        }
    }

    fn close(&self) {
        self.window.close();
        if let Some(app) = self.window.application() {
            app.quit();
        }
    }
}

fn flow() -> gtk::FlowBox {
    gtk::FlowBox::builder()
        .homogeneous(true)
        .min_children_per_line(2)
        .max_children_per_line(3)
        .selection_mode(gtk::SelectionMode::None)
        .column_spacing(12)
        .row_spacing(12)
        .build()
}

fn scrolled(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .child(child)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .min_content_height(380)
        .max_content_height(560)
        .propagate_natural_height(true)
        .build()
}

fn preview_icon(name: &str) -> gtk::Widget {
    let icon = gtk::Image::from_icon_name(name);
    icon.set_pixel_size(64);
    icon.upcast()
}
