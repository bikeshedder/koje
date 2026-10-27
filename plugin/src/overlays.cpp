// koje's overlays zoom (M8.86): when an overlay – a special workspace that
// is not a stash – opens, it is drawn 10 % larger and transparent and
// shrinks into place while it fades in, instead of Hyprland's slide; closing
// reverses it. The whole overlay is scaled as a picture – the windows'
// contents with their shadows and borders – around the monitor's centre:
// the windows themselves keep their places and sizes, so the apps are told
// nothing. The zoom follows the overlay's fade (`m_alpha`, Hyprland's
// `specialWorkspaceIn`/`Out` animation), which redraws the whole monitor
// while it runs. `plugin:koje:overlay_zoom = 0` turns it off.
#include "overlays.hpp"
#include "overview.hpp"

#include <hyprland/src/desktop/Workspace.hpp>
#include <hyprland/src/helpers/Monitor.hpp>
#include <hyprland/src/managers/animation/DesktopAnimationManager.hpp>
#include <hyprland/src/render/OpenGL.hpp>
#include <hyprland/src/render/Renderer.hpp>
#include <hyprland/src/render/pass/RendererHintsPassElement.hpp>

using origStartWorkspace = void (*)(void*, PHLWORKSPACE, CDesktopAnimationManager::eAnimationType, bool, bool);
static CFunctionHook* g_startWorkspaceHook = nullptr;

// How much larger an overlay is drawn when it starts to open.
constexpr float ZOOM = 1.1F;

static bool zoomOn() {
    static auto* const* PZOOM = (Hyprlang::INT* const*)HyprlandAPI::getConfigValue(PHANDLE, "plugin:koje:overlay_zoom")->getDataStaticPtr();
    return **PZOOM;
}

static bool isOverlay(const PHLWORKSPACE& ws) {
    return ws && ws->m_isSpecialWorkspace && !ws->m_name.starts_with("special:stash-") && !ws->m_name.starts_with("special:koje-");
}

// Hyprland slides a special workspace and fades it; the slide is cancelled,
// the fade drives the zoom.
static void hkStartWorkspace(void* thisptr, PHLWORKSPACE ws, CDesktopAnimationManager::eAnimationType type, bool left, bool instant) {
    ((origStartWorkspace)g_startWorkspaceHook->m_original)(thisptr, ws, type, left, instant);
    if (zoomOn() && !instant && isOverlay(ws))
        ws->m_renderOffset->setValueAndWarp(Vector2D{});
}

bool zoomOverlay(PHLMONITOR monitor, PHLWORKSPACE ws) {
    if (!monitor || !zoomOn() || !isOverlay(ws))
        return false;
    const float shown = ws->m_alpha->value();
    if (shown >= 1.F)
        return false;
    // Scaled around the monitor's centre: box × zoom + centre × (1 − zoom),
    // in the monitor's pixels as Hyprland draws.
    const float      zoom   = 1.F + (ZOOM - 1.F) * (1.F - shown);
    const Vector2D   centre = monitor->m_transformedSize / 2.0;
    SRenderModifData modif;
    modif.modifs.emplace_back(SRenderModifData::RMOD_TYPE_SCALE, zoom);
    modif.modifs.emplace_back(SRenderModifData::RMOD_TYPE_TRANSLATE, centre * (1.0 - zoom));
    g_pHyprRenderer->m_renderPass.add(makeUnique<CRendererHintsPassElement>(CRendererHintsPassElement::SData{modif}));
    return true;
}

// As Hyprland ends its own scaled drawing of a workspace.
void unzoomOverlay() {
    g_pHyprRenderer->m_renderPass.add(makeUnique<CRendererHintsPassElement>(CRendererHintsPassElement::SData{SRenderModifData{}}));
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
