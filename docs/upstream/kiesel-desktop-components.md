# Port priority: components needed by koje

Status: withdrawn, 2026-10-24 – koje's UI is built with GTK4 (koje ADR 0042).
Kept as the list of what koje's settings app needs.

koje isolates projects into contexts on Linux with Hyprland. Each context is a
rootless Podman container with its own name, color, saved states, window layouts
and memory limits. koje's settings app is the first application built on Kiesel
Desktop. This list orders the components and platform features to port by what
that app needs first.

## The settings app

A normal application window (xdg toplevel) on Hyprland, so only Wayland is
needed. It shows everything the user views and changes in koje:

- a list of contexts with color and status; start, shut down, pause, wake up
- per context the pages States, Layouts, Windows and Memory
- an editor for a context: name, color, access profile and overrides, mounted
  folders, memory limits
- later global settings

## General requirements

- **Theming:** light and dark, following the system setting.
- **Context color as accent:** every context has a user-chosen color (hex).
  The app uses it as the accent color for the selected context's pages,
  switched at runtime.
- **Icons:** Lucide, referenced by name. koje ships the Lucide icon font
  1.59.0 with its `codepoints.json`; the port should use that font or the same
  icon set at the same version.
- **Keyboard first:** every action is reachable by keyboard, the focus ring is
  visible, Esc closes dialogs.
- **No horizontal scrolling,** also in a narrow window.
- **Live data:** contexts, windows and memory figures change while the app is
  open (pushed by koje's background service over D-Bus). Components update in
  place without losing focus, selection or unsaved input.
- **Text:** English for now; all strings come from a catalog so a later
  language decision does not touch components.

## Priority 1: viewing and running contexts

### Platform

| Capability | Requirements from koje |
|---|---|
| Normal windows (xdg toplevel) | settable app-id and title, minimum size, remembers its size |
| Single instance | starting the app again brings the open window to the front, optionally on a given context (koje's bar opens it this way) |
| Updates from outside the UI | changes arrive over D-Bus; the app hands them to the UI from another thread or task and triggers a redraw |
| Input | pointer with left and right click and double-click; keyboard focus and navigation |
| Scaling | fractional scaling, crisp at 1×, 1.25×, 1.5×, 2× |
| Fonts | load fonts from files at runtime (Inter, a monospace font, the Lucide icon font) |

### Foundations

| Component | Requirements from koje |
|---|---|
| Tokens / theming | light and dark, per-subtree accent color |
| Icon | Lucide by name; `size`, `label` for icon-only use |
| Text | tones (muted info lines), `mono` (window IDs like `editor`), `tabular` (memory figures) |
| Stack, Cluster, Block, Divider | basic layout |

### Components

| Component | Used for | Requirements from koje |
|---|---|---|
| AppShell or SplitView | window frame: context list left, pages right | narrow window: the list collapses |
| Sidebar, or List + ListItem | context list | leading color stripe or dot, title, trailing status ("running", "paused", "off"), selected state, footer entry "+ New context", a section for global settings |
| ObjectHeader | top of a context's pages | color, name, short code, "running since 09:12", actions, tabs |
| Tabs | States · Layouts · Windows · Memory | a trailing slot for a summary ("4 windows · 3 layouts · 5.5 GB") would help |
| Button | Start, Pause, Wake up and other actions | `primary`, `outline`, `ghost`, `iconOnly`, `loading` (actions take seconds) |
| ButtonGroup + Menu | "Shut down ▾" | split button: main action plus a menu (shut down without saving, restart with state …) |
| Menu | context menus on contexts, states, windows | `MenuItem`, `MenuSeparator`, submenus, opened by right-click |
| Badge / Tag | context status, short code ("● KP") | custom hue from the context color; a muted variant for paused and off |
| Card | one card per saved state | `interactive`, selected state; slots for a layout thumbnail (see "Not in Kiesel yet"), title, info line, actions |
| Link | Load · Copy · Rename · Delete on a state card | inline, small |
| Table | layout matrix (windows × layouts), windows page | clickable and double-clickable header cells, a marked active column, a trailing "+ Layout" column, a footer row with checkboxes, a tinted row |
| Toggle or Checkbox | matrix cells, "Save automatically" | compact, inside table cells; two states: visible (●) and stashed (○) |
| Meter | memory per context | value, a marker for the soft limit, end of the bar = hard limit, `warn` from 90 % of the soft limit, label ("5.5 / 6 GB · 92 %", "paused") |
| DescriptionList | context details | image, access profile, mounts, limits |
| Tooltip | icon-only buttons | on hover and focus |
| Spinner / LoadingState | waiting for start, shut down, wake up | region placeholder with a delay |
| EmptyState | no contexts, no saved states | with an action |
| ErrorState, Alert | background service not reachable, a command failed | "Try again", dismissible |

## Priority 2: editing

### Platform

| Capability | Requirements from koje |
|---|---|
| Drag and drop inside a window | moving windows between layout thumbnails on the layouts page |

### Components

| Component | Used for | Requirements from koje |
|---|---|---|
| Dialog | new context, rename, confirm (recreate a context, delete a state) | focus trap, Esc, a destructive primary action |
| Form + ErrorSummary | context editor | unambiguous errors that focus the field; unsaved changes |
| UnsavedChangesBar | context editor | save or discard |
| SectionLayout | context editor with sections (general, access, mounts, memory) | sticky section navigation |
| TextField | names of contexts, states, layouts, window IDs | inline validation error |
| ColorField + ColorPicker | context color | hex in and out, presets |
| RadioGroup or Select | access profile | four options with descriptions: `full`, `desktop`, `headless`, `isolated` |
| Switch | access overrides and host interfaces | `gpu`, `sound`, `wayland`, `nested`, `network`, `notifications`, `file_chooser`, … in a list with descriptions |
| NumberField | soft and hard memory limit | units (MB, GB) with conversion |
| ListField | mounted folders | rows of folder paths, add and remove |
| Popover | small pickers ("Restart with state …") | anchored to a button |
| Toast | feedback after an action | action, timer |

## Later

| Component | Possible use |
|---|---|
| Sparkline, TimeChart | memory history per context |
| CommandPalette | search and actions in the app |
| ShortcutsDialog, Kbd | koje's key bindings |

## Not in Kiesel yet

koje needs these. They can live in koje, or in Kiesel Desktop if they are
general enough.

- **Layout thumbnail** (priority 1): a monitor drawn to its aspect ratio (e.g.
  5120 × 2160) with labelled window rectangles from relative geometry (x, y, w,
  h in 0–1), an optional highlight per window and a border in the context
  color. Used on state cards and on the layouts page. `LivePreview` scales a
  live UI, which is not the same.
- **Drag and drop between thumbnails** (priority 2): dragging a window
  rectangle from one layout thumbnail onto another adds the window to that
  layout.
- **Add tile** (priority 2): a dashed "+ New layout" placeholder next to the
  thumbnails.

## Not needed by koje

Communication (all), charts other than Meter and the later Sparkline and
TimeChart, the sign-in and account blocks, OtpInput, PasswordField, Rating,
Carousel, Calendar, the date and time fields, FilterBar, FilterBuilder,
ObjectEditor, ObjectTable, Dashboard, Tour, Hero.

## Open points

- The sub-parts of Tabs, Dialog, Sidebar, AppShell, ObjectHeader and
  SectionLayout were not checked against the web version's documentation.
