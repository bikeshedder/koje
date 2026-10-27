// koje's overlays zoom in (M8.86): when an overlay – a special workspace
// that is not a stash – opens, its windows start 10 % larger and
// transparent and shrink into place while they fade in, instead of
// Hyprland's slide. Only what is drawn moves: the windows' goals (and so
// the sizes the apps are told) stay as they are. Closing is Hyprland's own
// special workspace animation. `plugin:koje:overlay_zoom = 0` turns it off.
#include "overlays.hpp"
#include "overview.hpp"

#include <hyprland/src/Compositor.hpp>
#include <hyprland/src/desktop/Workspace.hpp>
#include <hyprland/src/desktop/view/Window.hpp>
#include <hyprland/src/managers/animation/DesktopAnimationManager.hpp>

using origStartWorkspace = void (*)(void*, PHLWORKSPACE, CDesktopAnimationManager::eAnimationType, bool, bool);
static CFunctionHook* g_startWorkspaceHook = nullptr;

// How much larger the windows start.
constexpr double ZOOM = 1.1;

static bool isOverlay(const PHLWORKSPACE& ws) {
    return ws && ws->m_isSpecialWorkspace && !ws->m_name.starts_with("special:stash-") && !ws->m_name.starts_with("special:koje-");
}

static void hkStartWorkspace(void* thisptr, PHLWORKSPACE ws, CDesktopAnimationManager::eAnimationType type, bool left, bool instant) {
    ((origStartWorkspace)g_startWorkspaceHook->m_original)(thisptr, ws, type, left, instant);
    static auto* const* PZOOM = (Hyprlang::INT* const*)HyprlandAPI::getConfigValue(PHANDLE, "plugin:koje:overlay_zoom")->getDataStaticPtr();
    if (!**PZOOM || instant || type != CDesktopAnimationManager::ANIMATION_TYPE_IN || !isOverlay(ws))
        return;
    // In place, at once: the windows do the moving.
    ws->m_renderOffset->setValueAndWarp(Vector2D{});
    ws->m_alpha->setValueAndWarp(1.F);
    for (const auto& w : g_pCompositor->m_windows) {
        if (!w || !w->m_isMapped || w->m_workspace != ws)
            continue;
        const Vector2D size = w->m_realSize->goal();
        const Vector2D pos  = w->m_realPosition->goal();
        const Vector2D big  = size * ZOOM;
        w->m_realSize->setValue(big);
        w->m_realPosition->setValue(pos - (big - size) / 2.0);
        w->m_alpha->setValue(0.F);
    }
}

bool hookOverlayZoom() {
    for (const auto& f : HyprlandAPI::findFunctionsByName(PHANDLE, "startAnimation")) {
        if (f.demangled.starts_with("CDesktopAnimationManager::startAnimation(") && f.demangled.contains("CWorkspace")) {
            g_startWorkspaceHook = HyprlandAPI::createFunctionHook(PHANDLE, f.address, (void*)hkStartWorkspace);
            return g_startWorkspaceHook && g_startWorkspaceHook->hook();
        }
    }
    return false;
}
