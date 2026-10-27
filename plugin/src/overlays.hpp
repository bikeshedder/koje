// Overlays zoom in and out (M8.86): see overlays.cpp.
#pragma once

#define WLR_USE_UNSTABLE

#include <hyprland/src/plugins/PluginAPI.hpp>
#include <hyprland/src/desktop/DesktopTypes.hpp>

// Hook Hyprland's workspace animation; false when it cannot.
bool hookOverlayZoom();
// Before a workspace's windows are drawn: scales what follows while an
// overlay opens or closes; true when it did – then `unzoomOverlay` after.
bool zoomOverlay(PHLMONITOR monitor, PHLWORKSPACE ws);
void unzoomOverlay();
