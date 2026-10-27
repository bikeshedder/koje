//! The settings app's Desktop page (ADR 0056): `[desktop]` in `config.toml`
//! – the windows' borders, gaps, corners, shadows and snapping, how an open
//! overlay darkens, blurs and zooms, the overview's animation. Save writes
//! `config.toml` through `koje config --stdin`; kojed applies it. Values
//! equal to koje's defaults are left unset (`DesktopConfig::from_values`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4 as gtk;
use gtk4::prelude::*;
use koje_core::config::GlobalConfig;
use koje_core::desktop::{self, Desktop, DesktopConfig};
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::settings_ui::{Ui, clear_page};

/// Shown for the sidebar's Desktop row.
pub(crate) struct DesktopPage {
    pub(crate) page: adw::PreferencesPage,
    /// The values the page was built from; it is rebuilt when they change
    /// and the user changed nothing.
    built: Rc<RefCell<Option<Desktop>>>,
    dirty: Rc<Cell<bool>>,
}

type Form = Rc<RefCell<Desktop>>;

impl DesktopPage {
    pub(crate) fn new() -> Self {
        Self {
            page: adw::PreferencesPage::new(),
            built: Rc::default(),
            dirty: Rc::default(),
        }
    }

    pub(crate) fn render(&self, ui: &Rc<Ui>, global: &GlobalConfig) {
        let values = global.desktop.resolve();
        if self.dirty.get() || *self.built.borrow() == Some(values) {
            return;
        }
        self.built.replace(Some(values));
        clear_page(&self.page);
        let form: Form = Rc::new(RefCell::new(values));
        let changed: Rc<dyn Fn()> = {
            let dirty = self.dirty.clone();
            Rc::new(move || dirty.set(true))
        };
        let spin = |title: &str, subtitle: &str, key: &str, field: fn(&mut Desktop) -> &mut u32| {
            spin(&form, &changed, title, subtitle, key, field)
        };
        let switch = |title: &str, subtitle: &str, field: fn(&mut Desktop) -> &mut bool| {
            switch(&form, &changed, title, subtitle, field)
        };

        let windows = adw::PreferencesGroup::builder()
            .title("Windows")
            .description("As in GNOME by default: no frame, no gaps, rounded corners.")
            .build();
        windows.add(&spin(
            "Border Width",
            "In the context's color, on every context window; 0: none, as in GNOME",
            "border_width",
            |d| &mut d.border_width,
        ));
        windows.add(&spin(
            "Gaps Between Windows",
            "Pixels",
            "gaps_inside",
            |d| &mut d.gaps_inside,
        ));
        windows.add(&spin(
            "Gaps at the Screen's Edges",
            "Pixels",
            "gaps_outside",
            |d| &mut d.gaps_outside,
        ));
        windows.add(&spin(
            "Rounded Corners",
            "Radius in pixels; 0: square",
            "corner_radius",
            |d| &mut d.corner_radius,
        ));
        windows.add(&switch("Shadows", "", |d| &mut d.shadows));
        windows.add(&switch(
            "Snap",
            "Moved windows snap to screen edges and other windows",
            |d| &mut d.snap,
        ));
        self.page.add(&windows);

        let foreign = adw::PreferencesGroup::builder()
            .title("Windows Outside Their Context")
            .description("A context's window on another context's workspace or a host workspace.")
            .build();
        foreign.add(&switch("Glow", "Its shadow in the context's color", |d| {
            &mut d.foreign_glow
        }));
        foreign.add(&switch(
            "Badge",
            "The context's name on its top edge",
            |d| &mut d.foreign_badge,
        ));
        self.page.add(&foreign);

        let overlays = adw::PreferencesGroup::builder()
            .title("Overlays")
            .description("What happens behind an open overlay (the stash looks the same). Their names, icons and keys are on the Workspaces page.")
            .build();
        overlays.add(&spin(
            "Darken the Background",
            "Percent; 0: not at all",
            "overlay_dim",
            |d| &mut d.overlay_dim,
        ));
        overlays.add(&spin(
            "Blur the Background",
            "Strength; 0: no blur",
            "overlay_blur",
            |d| &mut d.overlay_blur,
        ));
        overlays.add(&switch(
            "Zoom In",
            "Overlays zoom in when they open and out when they close; off: they slide",
            |d| &mut d.overlay_zoom,
        ));
        self.page.add(&overlays);

        let overview = adw::PreferencesGroup::builder().title("Overview").build();
        overview.add(&spin(
            "Animation",
            "Milliseconds the windows take to fly in; 0: at once",
            "overview_duration",
            |d| &mut d.overview_duration,
        ));
        self.page.add(&overview);
        self.page.add(&self.save(ui, &form));
    }

    fn save(&self, ui: &Rc<Ui>, form: &Form) -> adw::PreferencesGroup {
        let group = adw::PreferencesGroup::new();
        let buttons = gtk::Box::builder()
            .spacing(12)
            .halign(gtk::Align::End)
            .build();
        let defaults = gtk::Button::builder()
            .label("Restore Defaults")
            .css_classes(["pill"])
            .build();
        let save = gtk::Button::builder()
            .label("Save")
            .css_classes(["suggested-action", "pill"])
            .build();
        let write = {
            let (ui, dirty, built) = (Rc::downgrade(ui), self.dirty.clone(), self.built.clone());
            move |values: &Desktop| {
                let Some(ui) = ui.upgrade() else { return };
                match new_text(&DesktopConfig::from_values(values)) {
                    Ok(text) => {
                        ui.command("config.toml", &["config", "--stdin"], Some(text));
                        // Built again from the file as saved.
                        dirty.set(false);
                        built.replace(None);
                    }
                    Err(e) => ui.toast(&e),
                }
            }
        };
        let write = Rc::new(write);
        {
            let (write, form) = (write.clone(), form.clone());
            save.connect_clicked(move |_| write(&form.borrow()));
        }
        defaults.connect_clicked(move |_| write(&desktop::DEFAULT));
        buttons.append(&defaults);
        buttons.append(&save);
        group.add(&buttons);
        group
    }
}

fn spin(
    form: &Form,
    changed: &Rc<dyn Fn()>,
    title: &str,
    subtitle: &str,
    key: &str,
    field: fn(&mut Desktop) -> &mut u32,
) -> adw::SpinRow {
    let (min, max) = desktop::RANGES
        .iter()
        .find(|(k, _, _)| *k == key)
        .map_or((0, 100), |(_, min, max)| (*min, *max));
    let row = adw::SpinRow::with_range(f64::from(min), f64::from(max), 1.0);
    row.set_title(title);
    row.set_subtitle(subtitle);
    row.set_value(f64::from(*field(&mut form.borrow_mut())));
    let (form, changed) = (form.clone(), changed.clone());
    row.connect_value_notify(move |r| {
        // The row keeps the value inside its range.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let v = r.value().round() as u32;
        *field(&mut form.borrow_mut()) = v;
        changed();
    });
    row
}

fn switch(
    form: &Form,
    changed: &Rc<dyn Fn()>,
    title: &str,
    subtitle: &str,
    field: fn(&mut Desktop) -> &mut bool,
) -> adw::SwitchRow {
    let row = adw::SwitchRow::builder()
        .title(title)
        .active(*field(&mut form.borrow_mut()))
        .build();
    if !subtitle.is_empty() {
        row.set_subtitle(subtitle);
    }
    let (form, changed) = (form.clone(), changed.clone());
    row.connect_active_notify(move |r| {
        *field(&mut form.borrow_mut()) = r.is_active();
        changed();
    });
    row
}

/// `config.toml` as it is now (empty when missing), with `[desktop]` set.
fn new_text(cfg: &DesktopConfig) -> Result<String, String> {
    let paths = koje_core::paths::Paths::from_env().map_err(|e| e.to_string())?;
    let path = paths.global_config();
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("reading {}: {e}", path.display())),
    };
    desktop::with_desktop(&text, cfg).map_err(|e| e.to_string())
}
