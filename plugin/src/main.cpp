// koje's Hyprland plugin (ADR 0051). New windows open as in GNOME (ADR 0052,
// windows.cpp); windows move by their title bars (drag.cpp). And the
// overview's windows:
//
//   hyprctl dispatch koje:overview open <monitor> <address>:<x>,<y>,<w>,<h> …
//   hyprctl dispatch koje:overview update <monitor> <address>:<x>,<y>,<w>,<h> …
//   hyprctl dispatch koje:overview close [<monitor>]
//
// `open` flies the windows from their places into the given ones, `update`
// lets those shown glide there (new ones appear there), `close` flies them
// back. Places are logical
// pixels relative to koje-shell's overview surface on that monitor.
#define WLR_USE_UNSTABLE

#include "drag.hpp"
#include "foreign.hpp"
#include "overlays.hpp"
#include "overview.hpp"
#include "pass.hpp"
#include "windows.hpp"

#include <hyprland/src/Compositor.hpp>
#include <hyprland/src/desktop/view/Window.hpp>
#include <hyprland/src/helpers/Monitor.hpp>
#include <hyprland/src/render/OpenGL.hpp>
#include <hyprland/src/render/Renderer.hpp>

#include <sstream>

static CFunctionHook* g_renderWindowsHook   = nullptr;
static CFunctionHook* g_renderFullscreenHook = nullptr;
static CFunctionHook* g_addDamageBoxHook    = nullptr;
static CFunctionHook* g_addDamageRegionHook = nullptr;
static CFunctionHook* g_updateStateHook     = nullptr;

using origRenderWindows   = void (*)(void*, PHLMONITOR, PHLWORKSPACE, const Time::steady_tp&);
using origUpdateState     = void (*)(void*);
using origAddDamageBox    = void (*)(void*, const CBox&);
using origAddDamageRegion = void (*)(void*, const pixman_region32_t*);

APICALL EXPORT std::string PLUGIN_API_VERSION() {
    return HYPRLAND_API_VERSION;
}

static COverview* overviewOf(const CMonitor* monitor) {
    if (!monitor)
        return nullptr;
    const auto it = g_overviews.find(monitor->m_name);
    return it == g_overviews.end() ? nullptr : it->second.get();
}

// Only the windows are replaced: the wallpaper and the layers below them and
// above (the bar, the shell's overview) are drawn as always. Windows of a
// shown special workspace (a stash, an overlay) are left out meanwhile. With
// one shown, Hyprland darkens and blurs after the workspace's windows: the
// overview waits until all windows are drawn (`RENDER_POST_WINDOWS`), so it
// lies sharp on top – showing the overlay's windows, as the shell lists them.
static bool replaceWindows(PHLMONITOR monitor, PHLWORKSPACE workspace) {
    const auto O = overviewOf(monitor.get());
    if (!O)
        return false;
    if (!workspace || !workspace->m_isSpecialWorkspace) {
        if (monitor->m_specialFade->value() > 0.F)
            O->deferred = true;
        else
            O->render();
    }
    return true;
}

// Hyprland's drawing of a workspace's windows; an opening or closing
// overlay zoomed (overlays.cpp).
static void renderWindows(CFunctionHook* hook, void* thisptr, PHLMONITOR monitor, PHLWORKSPACE workspace, const Time::steady_tp& now) {
    if (replaceWindows(monitor, workspace))
        return;
    const bool zoomed = zoomOverlay(monitor, workspace);
    ((origRenderWindows)hook->m_original)(thisptr, monitor, workspace, now);
    if (zoomed)
        unzoomOverlay();
    if (workspace && workspace->m_isSpecialWorkspace && monitor->m_specialFade->value() > 0.F)
        g_pHyprRenderer->m_renderPass.add(makeUnique<CKojeNoCullPassElement>());
}

static void hkRenderWindows(void* thisptr, PHLMONITOR monitor, PHLWORKSPACE workspace, const Time::steady_tp& now) {
    renderWindows(g_renderWindowsHook, thisptr, monitor, workspace, now);
}

static void hkRenderFullscreen(void* thisptr, PHLMONITOR monitor, PHLWORKSPACE workspace, const Time::steady_tp& now) {
    renderWindows(g_renderFullscreenHook, thisptr, monitor, workspace, now);
}

// Behind a shown overlay (or stash) Hyprland blurs what this frame drew
// (`blur:special`). With only part of the monitor damaged, that blur reads
// the last frame around the damage – already blurred, dimmed and holding the
// overlay's windows – so it smeared around the cursor and the windows and
// let the workspace show through. The whole monitor is redrawn instead, as
// Hyprland does while the cursor zooms: the workspace first, blurred, the
// overlay on top.
static bool wholeMonitor(CMonitor* monitor) {
    if (!monitor || monitor->m_specialFade->value() <= 0.F)
        return false;
    monitor->m_damage.damageEntire();
    g_pCompositor->scheduleFrameForMonitor(monitor->m_self.lock(), Aquamarine::IOutput::AQ_SCHEDULE_DAMAGE);
    return true;
}

static void hkAddDamageBox(void* thisptr, const CBox& box) {
    const auto O = overviewOf((CMonitor*)thisptr);
    if (O && !O->blockDamage) {
        O->onDamage();
        return;
    }
    if (wholeMonitor((CMonitor*)thisptr))
        return;
    ((origAddDamageBox)g_addDamageBoxHook->m_original)(thisptr, box);
}

static void hkAddDamageRegion(void* thisptr, const pixman_region32_t* region) {
    const auto O = overviewOf((CMonitor*)thisptr);
    if (O && !O->blockDamage) {
        O->onDamage();
        return;
    }
    if (wholeMonitor((CMonitor*)thisptr))
        return;
    ((origAddDamageRegion)g_addDamageRegionHook->m_original)(thisptr, region);
}

static void hkUpdateState(void* thisptr) {
    onWindowState((Desktop::View::CWindow*)thisptr);
    ((origUpdateState)g_updateStateHook->m_original)(thisptr);
}

// `0x55d0e8c2a1b0:10,20,800,600`
static std::optional<std::pair<PHLWINDOW, CBox>> parsePlace(const std::string& word) {
    const auto colon = word.find(':');
    if (colon == std::string::npos)
        return std::nullopt;
    try {
        const auto      handle = std::stoull(word.substr(0, colon), nullptr, 16);
        const auto      window = g_pCompositor->getWindowFromHandle((uint32_t)(handle & 0xFFFFFFFF));
        std::istringstream in{word.substr(colon + 1)};
        double          v[4];
        char            comma;
        if (!window || !(in >> v[0] >> comma >> v[1] >> comma >> v[2] >> comma >> v[3]))
            return std::nullopt;
        return std::pair{window, CBox{v[0], v[1], v[2], v[3]}};
    } catch (...) { return std::nullopt; }
}

static SDispatchResult onOverview(std::string arg) {
    std::istringstream       in{arg};
    std::vector<std::string> words;
    for (std::string w; in >> w;)
        words.push_back(w);
    if (words.empty())
        return {.success = false, .error = "koje:overview: open, update or close"};

    if (words[0] == "close") {
        for (auto& [name, overview] : g_overviews) {
            if (words.size() < 2 || words[1] == name)
                overview->close();
        }
        return {};
    }
    if ((words[0] != "open" && words[0] != "update") || words.size() < 2)
        return {.success = false, .error = "koje:overview: open|update <monitor> <address>:<x>,<y>,<w>,<h> …"};

    const auto monitor = g_pCompositor->getMonitorFromName(words[1]);
    if (!monitor)
        return {.success = false, .error = "koje:overview: no monitor " + words[1]};
    std::vector<std::pair<PHLWINDOW, CBox>> places;
    for (size_t i = 2; i < words.size(); ++i) {
        if (const auto p = parsePlace(words[i]))
            places.push_back(*p);
    }
    auto& overview = g_overviews[monitor->m_name];
    // A closing one is replaced: the shell opened it again.
    const bool fresh = !overview || overview->closing;
    if (fresh)
        overview = makeUnique<COverview>(monitor);
    overview->setWindows(places, fresh && words[0] == "open");
    return {};
}

static void fail(const std::string& reason) {
    HyprlandAPI::addNotification(PHANDLE, "[koje] " + reason, CHyprColor{1.0, 0.2, 0.2, 1.0}, 8000);
    throw std::runtime_error("[koje] " + reason);
}

// `name` is a mangled symbol, or a demangled one up to its `(`: Hyprland
// matches parts of names (`renderWorkspace` finds `renderWorkspaceWindows`
// too), so the right one is picked here.
static CFunctionHook* hook(const std::string& name, void* to) {
    for (const auto& f : HyprlandAPI::findFunctionsByName(PHANDLE, name.contains('(') ? name.substr(name.rfind("::") + 2, name.find('(') - name.rfind("::") - 2) : name)) {
        if (f.signature == name || f.demangled.starts_with(name))
            return HyprlandAPI::createFunctionHook(PHANDLE, f.address, to);
    }
    fail("Hyprland has no " + name + " – the plugin was built for another version");
    return nullptr;
}

APICALL EXPORT PLUGIN_DESCRIPTION_INFO PLUGIN_INIT(HANDLE handle) {
    PHANDLE = handle;

    const std::string running = __hyprland_api_get_hash(), built = __hyprland_api_get_client_hash();
    if (running != built)
        fail("built for Hyprland " + built + ", this one is " + running + " – koje's package is built against Ubuntu's hyprland-dev");

    g_renderWindowsHook    = hook("CHyprRenderer::renderWorkspaceWindows(", (void*)hkRenderWindows);
    g_renderFullscreenHook = hook("CHyprRenderer::renderWorkspaceWindowsFullscreen(", (void*)hkRenderFullscreen);
    g_addDamageBoxHook    = hook("_ZN8CMonitor9addDamageERKN9Hyprutils4Math4CBoxE", (void*)hkAddDamageBox);
    g_addDamageRegionHook = hook("_ZN8CMonitor9addDamageEPK15pixman_region32", (void*)hkAddDamageRegion);
    g_updateStateHook     = hook("Desktop::View::CWindow::onUpdateState(", (void*)hkUpdateState);
    if (!findRenderWindow())
        fail("Hyprland has no CHyprRenderer::renderWindow – the plugin was built for another version");
    if (!g_renderWindowsHook->hook() || !g_renderFullscreenHook->hook() || !g_addDamageBoxHook->hook() || !g_addDamageRegionHook->hook())
        fail("could not hook Hyprland's rendering");
    if (!g_updateStateHook->hook())
        fail("could not hook Hyprland's window states");
    // Without it overlays open as Hyprland animates them: no reason to fail.
    if (!hookOverlayZoom())
        HyprlandAPI::addNotification(PHANDLE, "[koje] overlays cannot zoom: Hyprland's workspace animation was not found", CHyprColor{1.0, 0.6, 0.2, 1.0}, 5000);
    // Without it foreign windows only lack the glow: no reason to fail.
    if (!hookForeign())
        HyprlandAPI::addNotification(PHANDLE, "[koje] windows outside their context cannot glow: Hyprland's functions were not found", CHyprColor{1.0, 0.6, 0.2, 1.0}, 5000);
    for (const auto& w : g_pCompositor->m_windows) {
        addBadge(w);
        letMove(w);
    }

    static auto openWindow = HyprlandAPI::registerCallbackDynamic(PHANDLE, "openWindow", [](void*, SCallbackInfo&, std::any data) {
        try {
            const auto w = std::any_cast<PHLWINDOW>(data);
            onWindowOpen(w);
            addBadge(w);
            letMove(w);
        } catch (const std::bad_any_cast&) {}
    });

    static auto mouseButton = HyprlandAPI::registerCallbackDynamic(PHANDLE, "mouseButton", [](void*, SCallbackInfo&, std::any data) {
        try {
            onMouseButton(std::any_cast<IPointer::SButtonEvent>(data));
        } catch (const std::bad_any_cast&) {}
    });

    static auto preRender = HyprlandAPI::registerCallbackDynamic(PHANDLE, "preRender", [](void*, SCallbackInfo&, std::any data) {
        try {
            if (const auto O = overviewOf(std::any_cast<PHLMONITOR>(data).get()))
                O->onPreRender();
        } catch (const std::bad_any_cast&) {
            for (auto& [_, overview] : g_overviews)
                overview->onPreRender();
        }
    });

    static auto render = HyprlandAPI::registerCallbackDynamic(PHANDLE, "render", [](void*, SCallbackInfo&, std::any data) {
        try {
            if (std::any_cast<eRenderStage>(data) != RENDER_POST_WINDOWS)
                return;
            const auto O = overviewOf(g_pHyprOpenGL->m_renderData.pMonitor.get());
            if (O && O->deferred) {
                O->deferred = false;
                O->render();
            }
        } catch (const std::bad_any_cast&) {}
    });

    HyprlandAPI::addDispatcherV2(PHANDLE, "koje:overview", ::onOverview);
    // GNOME's dark overview, drawn over the wallpaper as the windows fly in.
    HyprlandAPI::addConfigValue(PHANDLE, "plugin:koje:overview_background", Hyprlang::INT{0xFF1D1D20});
    // How long the windows fly, in ms (GNOME's overview: 250); 0: at once.
    HyprlandAPI::addConfigValue(PHANDLE, "plugin:koje:overview_duration", Hyprlang::INT{250});
    // New windows larger than 80 % of the screen open maximized, as GNOME's
    // auto-maximize (ADR 0052); 0: off.
    HyprlandAPI::addConfigValue(PHANDLE, "plugin:koje:auto_maximize", Hyprlang::INT{1});
    // Overlays zoom in when they open and out when they close (M8.86); 0:
    // Hyprland's slide.
    HyprlandAPI::addConfigValue(PHANDLE, "plugin:koje:overlay_zoom", Hyprlang::INT{1});
    // Windows outside their context glow in its color and carry a badge with
    // its name (ADR 0057); 0: off.
    HyprlandAPI::addConfigValue(PHANDLE, "plugin:koje:foreign_glow", Hyprlang::INT{1});
    HyprlandAPI::addConfigValue(PHANDLE, "plugin:koje:foreign_badge", Hyprlang::INT{1});
    HyprlandAPI::reloadConfig();

    return {"koje", "koje's overview (the real windows, scaled), new windows as in GNOME, title bars moving windows, overlays zooming, windows outside their context marked", "koje", "1"};
}

APICALL EXPORT void PLUGIN_EXIT() {
    g_pHyprRenderer->m_renderPass.removeAllOfType("CKojeOverviewPassElement");
    g_pHyprRenderer->m_renderPass.removeAllOfType("CKojeNoCullPassElement");
    g_overviews.clear();
}
