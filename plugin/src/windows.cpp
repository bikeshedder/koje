#include "windows.hpp"
#include "overview.hpp"

#include <hyprland/src/Compositor.hpp>
#include <hyprland/src/desktop/view/Window.hpp>
#include <hyprland/src/helpers/Monitor.hpp>
#include <hyprland/src/protocols/XDGShell.hpp>

#include <algorithm>
#include <vector>

// Mutter's MAX_UNMAXIMIZED_WINDOW_AREA: larger new windows open maximized.
constexpr double AUTO_MAXIMIZE_AREA = 0.8;

// Wayland windows that asked to be maximized while not mapped yet.
static std::vector<PHLWINDOWREF> g_wantMaximized;

void onWindowState(Desktop::View::CWindow* window) {
    if (window->m_isMapped || !window->m_xdgSurface || !window->m_xdgSurface->m_toplevel)
        return;
    // Only set while the request is handled: Hyprland resets it afterwards.
    const auto requests = window->m_xdgSurface->m_toplevel->m_state.requestsMaximize;
    if (!requests.has_value())
        return;
    std::erase_if(g_wantMaximized, [window](const auto& w) { return !w || w.get() == window; });
    if (*requests)
        g_wantMaximized.push_back(window->m_self);
}

// Mutter maximizes only windows that can be resized.
static bool fixedSize(const PHLWINDOW& window) {
    if (!window->m_xdgSurface || !window->m_xdgSurface->m_toplevel)
        return false;
    const auto& limits = window->m_xdgSurface->m_toplevel->m_current;
    return limits.maxSize.x > 0 && limits.maxSize.y > 0 && limits.minSize == limits.maxSize;
}

static bool nearlyScreenSized(const PHLWINDOW& window) {
    static auto* const* PAUTO = (Hyprlang::INT* const*)HyprlandAPI::getConfigValue(PHANDLE, "plugin:koje:auto_maximize")->getDataStaticPtr();
    const auto          monitor = window->m_monitor.lock();
    if (!**PAUTO || !monitor || fixedSize(window))
        return false;
    const auto area = monitor->logicalBoxMinusReserved();
    const auto size = window->m_realSize->goal();
    return size.x * size.y > area.w * area.h * AUTO_MAXIMIZE_AREA;
}

void onWindowOpen(PHLWINDOW window) {
    const bool asked = std::ranges::any_of(g_wantMaximized, [&](const auto& w) { return w.lock() == window; });
    std::erase_if(g_wantMaximized, [&](const auto& w) { return !w || w.lock() == window; });

    if (!window->m_isFloating || window->isFullscreen() || (window->m_suppressedEvents & Desktop::View::SUPPRESS_MAXIMIZE))
        return;
    // As Hyprland does for a maximize request of a mapped window: the
    // program is told it is maximized.
    if (asked || nearlyScreenSized(window))
        g_pCompositor->changeWindowFullscreenModeClient(window, FSMODE_MAXIMIZED, true);
}
