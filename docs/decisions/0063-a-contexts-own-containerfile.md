# 0063. A context's own Containerfile, opened with xdg-open

- Status: Accepted
- Date: 2026-11-10
- Source: discussion (user, 2026-11-10: "easily create a Dockerfile for a
  context and edit it in the user's preferred editor"); ADR 0040; plan G.37

## Context

ADR 0040 lets a context use an image folder (`~/.config/koje/images/<name>/`
with a `Containerfile`), but making one means creating the folder, finding the
right `FROM` line and changing `image = …` by hand. Users want one step from a
context to a Containerfile they can edit, in the editor they already use, from
the CLI and from koje Settings.

## Decision

- ADR 0040 is accepted. Its image references, user layer and image folders
  (M3.18–M3.20) are built first; this decision builds on them.
- **A context's own image** is the image folder named after the context,
  `~/.config/koje/images/<context>/`. Asking for it (CLI or settings app):
  - if the context already uses an image folder, that folder's
    `Containerfile` is opened (it may be shared with other contexts);
  - else the folder is created (unless it exists) with a `Containerfile`
    that starts `FROM` the context's current image and explains in comments
    what to add; then `image = "<context>"` is written to `context.toml`.
    The new image therefore holds what the context had, until it is edited.
  - A context named like a built-in image (`fedora`) gets `<context>-own`,
    so that it does not take the built-in image's place for all contexts.
- **Editing** opens the file with the desktop's default application for it:
  `xdg-open` from the CLI (`koje image edit <context>`), `Gtk.FileLauncher`
  (the same, through the portal) from the settings app. koje has no editor
  setting.
- **Building**: an image folder's image carries the hash of the folder's
  files (`io.koje.source`) and the IDs of its parents. When a context's
  container is created (the first `koje up`, `koje recreate`), koje builds
  the images that are missing or whose folder or parent changed, parents
  first, then the user layer. A build never touches an existing container
  (ADR 0040): for a stopped or running context whose image changed, `koje up`
  says so and asks to recreate, as for a changed access profile (M5.11); the
  settings app shows it and its Recreate builds and applies it.

## Consequences

+ From any context to an editable Containerfile in one step, in the user's
  own editor; the image survives recreates and can be shared later.
+ No new editor configuration: GNOME's default app for the file type.
- `xdg-open` picks the app for `text/plain`-like files; a user who wants
  another editor changes it in GNOME (Files → Open With).
- A build takes minutes inside a recreate; its lines are shown as the
  operation's progress.
