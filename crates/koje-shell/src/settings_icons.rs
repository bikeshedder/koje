//! The settings app's icon chooser (ADR 0055): a button showing the icon,
//! whose popover lists the icon theme's symbolic icons with a search, and
//! "Default" to unset it.

use std::rc::Rc;

use gtk4 as gtk;
use gtk4::prelude::*;

/// At most this many icons in the grid; the search narrows them.
const SHOWN: usize = 240;

/// A button with `current` (or `fallback` while unset); picking an icon
/// calls `on_pick` with its name, "Default" with an empty one.
pub(crate) fn icon_button(
    current: &str,
    fallback: &str,
    on_pick: impl Fn(String) + 'static,
) -> gtk::MenuButton {
    let shown = |name: &str| {
        if name.is_empty() {
            fallback.to_owned()
        } else {
            name.to_owned()
        }
    };
    let image = gtk::Image::from_icon_name(&shown(current));
    let button = gtk::MenuButton::builder()
        .child(&image)
        .valign(gtk::Align::Center)
        .tooltip_text("Choose an Icon")
        .css_classes(["flat"])
        .build();
    let search = gtk::SearchEntry::builder()
        .placeholder_text("Search icons")
        .build();
    let grid = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .max_children_per_line(8)
        .min_children_per_line(8)
        .homogeneous(true)
        .valign(gtk::Align::Start)
        .build();
    let default = gtk::Button::builder().label("Default").build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    content.append(&search);
    content.append(
        &gtk::ScrolledWindow::builder()
            .child(&grid)
            .min_content_height(280)
            .min_content_width(360)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build(),
    );
    content.append(&default);
    let popover = gtk::Popover::builder().child(&content).build();
    button.set_popover(Some(&popover));

    let fallback = fallback.to_owned();
    let pick: Rc<dyn Fn(String)> = {
        let (image, popover) = (image.clone(), popover.clone());
        Rc::new(move |name: String| {
            image.set_icon_name(Some(if name.is_empty() { &fallback } else { &name }));
            popover.popdown();
            on_pick(name);
        })
    };
    {
        let pick = pick.clone();
        default.connect_clicked(move |_| pick(String::new()));
    }
    let fill = {
        let (grid, search, pick) = (grid.clone(), search.clone(), pick.clone());
        move || fill(&grid, &search.text(), &pick)
    };
    {
        let fill = fill.clone();
        search.connect_search_changed(move |_| fill());
    }
    // Read the theme only when the chooser opens.
    popover.connect_show(move |_| fill());
    button
}

/// The theme's symbolic icons matching `query`, sorted.
fn names(query: &str) -> Vec<String> {
    let Some(display) = gtk::gdk::Display::default() else {
        return Vec::new();
    };
    let query = query.trim().to_lowercase();
    let mut names: Vec<String> = gtk::IconTheme::for_display(&display)
        .icon_names()
        .into_iter()
        .map(String::from)
        .filter(|n| n.ends_with("-symbolic") && !n.contains("-rtl") && n.contains(query.as_str()))
        .collect();
    names.sort();
    names.dedup();
    names
}

fn fill(grid: &gtk::FlowBox, query: &str, pick: &Rc<dyn Fn(String)>) {
    grid.remove_all();
    for name in names(query).into_iter().take(SHOWN) {
        let b = gtk::Button::builder()
            .icon_name(&name)
            .tooltip_text(&name)
            .css_classes(["flat"])
            .build();
        let pick = pick.clone();
        b.connect_clicked(move |_| pick(name.clone()));
        grid.append(&b);
    }
}
