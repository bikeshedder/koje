// New windows as in GNOME (ADR 0052): one that asked to be maximized before
// it was shown opens maximized – Hyprland 0.53 drops that request – and one
// nearly as large as the screen too, as Mutter's auto-maximize does.
#pragma once

#define WLR_USE_UNSTABLE

#include <hyprland/src/desktop/DesktopTypes.hpp>

// From the hook of CWindow::onUpdateState, before Hyprland's own: notes a
// maximize request of a window not mapped yet.
void onWindowState(Desktop::View::CWindow* window);

// Hyprland's `openWindow`: mapped, before its first frame.
void onWindowOpen(PHLWINDOW window);
