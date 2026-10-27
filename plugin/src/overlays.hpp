// Overlays zoom in (M8.86): see overlays.cpp.
#pragma once

#define WLR_USE_UNSTABLE

#include <hyprland/src/plugins/PluginAPI.hpp>

// Hook Hyprland's workspace animation; false when it cannot.
bool hookOverlayZoom();
