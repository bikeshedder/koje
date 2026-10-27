// Windows move by their title bars (M8.95): see drag.cpp.
#pragma once

#define WLR_USE_UNSTABLE

#include <hyprland/src/plugins/PluginAPI.hpp>
#include <hyprland/src/desktop/DesktopTypes.hpp>
#include <hyprland/src/devices/IPointer.hpp>

// Answer the window's requests to be moved; called when it opens.
void letMove(PHLWINDOW window);
// Every pointer button pressed or released.
void onMouseButton(const IPointer::SButtonEvent& e);
