# 0058. Notifications are grouped by app and compact, as in Android

- Status: Accepted
- Date: 2026-10-28
- Source: discussion (requested 2026-10-28: "Group the notifications by app
  similar to how Android manages notifications. Make the display more compact.
  The design doesn't have to follow the GNOME notifications."); plan task M8.99.
  Amends ADR 0043 ("OSD and notifications look and sit like GNOME's") for
  notifications.

## Context

M8.36 and M8.81 drew notifications exactly as GNOME Shell 50 does: one large
card per notification (48 px picture, a 24 px header, 12 px between cards), in
the date menu only a heading over several cards of one app. A few chatty apps
fill the list, and each card takes much space.

## Decision

- **One card per app and context.** The date menu's list holds a card per app;
  the same app in two contexts is two cards (each with its context's stripe and
  name). Cards are ordered by their newest notification.
- **Collapsed by default**, as Android shows a group: the header (app icon, app
  name, context, the newest one's time, how many there are), the newest
  notification in full with its buttons, the next two as one line each
  ("**title** body"), then "+N more". A card with a critical notification starts
  expanded.
- **Expanded** (the chevron, or a click on the header or the lines): every
  notification of the app, each with its own short time and close button, the
  body up to six lines. The shell keeps what the user expanded or collapsed
  while the group is there.
- **The header's close button dismisses the whole group.**
- **Compact:** 32 px pictures, a 22 px header, short times (`now`, `5m`, `3h`,
  `2d`), cards 6 px apart. The banner is the same card with one notification
  and a two-line body.
- GNOME stays the source of what is not about the look: the server's behavior
  (urgency, Do Not Disturb, the banner queue, transient and resident,
  activation), the markup allowed in bodies, the colors in light and dark, the
  date menu with the calendar.

## Consequences

- (+) Many notifications of one app take one card; the list stays short.
- (+) The grouping is a pure function (`notifications::groups`), tested.
- (-) koje's notifications no longer look like GNOME's; screenshots of GNOME
  are no longer the reference for them.
