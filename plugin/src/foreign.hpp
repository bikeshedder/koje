// Windows outside their context (ADR 0057): see foreign.cpp.
#pragma once

#define WLR_USE_UNSTABLE

#include <hyprland/src/plugins/PluginAPI.hpp>

// Hook Hyprland's decoration values and window moves; false when it cannot.
bool hookForeign();
// Every window gets a badge; it is drawn only while the window is foreign.
void addBadge(PHLWINDOW window);
