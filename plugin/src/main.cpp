// koje's Hyprland plugin (ADR 0051). For now the overview's windows:
//
//   hyprctl dispatch koje:overview open <monitor> <address>:<x>,<y>,<w>,<h> …
//   hyprctl dispatch koje:overview update <monitor> <address>:<x>,<y>,<w>,<h> …
//   hyprctl dispatch koje:overview close [<monitor>]
//
// `open` flies the windows from their places into the given ones, `update`
// moves them there at once, `close` flies them back. Places are logical
// pixels relative to koje-shell's overview surface on that monitor.
#define WLR_USE_UNSTABLE

#include "overview.hpp"
#include "pass.hpp"

#include <hyprland/src/Compositor.hpp>
#include <hyprland/src/desktop/view/LayerSurface.hpp>
#include <hyprland/src/desktop/view/Window.hpp>
#include <hyprland/src/helpers/Monitor.hpp>
#include <hyprland/src/render/Renderer.hpp>

#include <sstream>

static CFunctionHook* g_renderWorkspaceHook = nullptr;
static CFunctionHook* g_addDamageBoxHook    = nullptr;
static CFunctionHook* g_addDamageRegionHook = nullptr;

using origRenderWorkspace = void (*)(void*, PHLMONITOR, PHLWORKSPACE, const Time::steady_tp&, const CBox&);
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

static void hkRenderWorkspace(void* thisptr, PHLMONITOR monitor, PHLWORKSPACE workspace, const Time::steady_tp& now, const CBox& geometry) {
    if (const auto O = overviewOf(monitor.get())) {
        O->render();
        return;
    }
    ((origRenderWorkspace)g_renderWorkspaceHook->m_original)(thisptr, monitor, workspace, now, geometry);
}

static void hkAddDamageBox(void* thisptr, const CBox& box) {
    const auto O = overviewOf((CMonitor*)thisptr);
    if (O && !O->blockDamage) {
        O->onDamage();
        return;
    }
    ((origAddDamageBox)g_addDamageBoxHook->m_original)(thisptr, box);
}

static void hkAddDamageRegion(void* thisptr, const pixman_region32_t* region) {
    const auto O = overviewOf((CMonitor*)thisptr);
    if (O && !O->blockDamage) {
        O->onDamage();
        return;
    }
    ((origAddDamageRegion)g_addDamageRegionHook->m_original)(thisptr, region);
}

// The shell's overview surface on the monitor: places are relative to it.
static Vector2D shellOrigin(PHLMONITOR monitor) {
    for (const auto& layer : monitor->m_layerSurfaceLayers) {
        for (const auto& ref : layer) {
            const auto LS = ref.lock();
            if (LS && LS->m_mapped && LS->m_namespace == SHELL_NAMESPACE)
                return LS->m_geometry.pos() - monitor->m_position;
        }
    }
    return {};
}

// `0x55d0e8c2a1b0:10,20,800,600`
static std::optional<std::pair<PHLWINDOW, CBox>> parsePlace(const std::string& word, const Vector2D& origin) {
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
        return std::pair{window, CBox{v[0] + origin.x, v[1] + origin.y, v[2], v[3]}};
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
    const auto                                origin = shellOrigin(monitor);
    std::vector<std::pair<PHLWINDOW, CBox>> places;
    for (size_t i = 2; i < words.size(); ++i) {
        if (const auto p = parsePlace(words[i], origin))
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

static CFunctionHook* hook(const std::string& name, void* to) {
    const auto found = HyprlandAPI::findFunctionsByName(PHANDLE, name);
    if (found.empty())
        fail("Hyprland has no " + name + " – the plugin was built for another version");
    return HyprlandAPI::createFunctionHook(PHANDLE, found[0].address, to);
}

APICALL EXPORT PLUGIN_DESCRIPTION_INFO PLUGIN_INIT(HANDLE handle) {
    PHANDLE = handle;

    if (__hyprland_api_get_hash() != __hyprland_api_get_client_hash())
        fail("built for another Hyprland version – rebuild koje (just deb)");

    g_renderWorkspaceHook = hook("renderWorkspace", (void*)hkRenderWorkspace);
    g_addDamageBoxHook    = hook("_ZN8CMonitor9addDamageERKN9Hyprutils4Math4CBoxE", (void*)hkAddDamageBox);
    g_addDamageRegionHook = hook("addDamageEPK15pixman_region32", (void*)hkAddDamageRegion);
    if (!findRenderWindow())
        fail("Hyprland has no CHyprRenderer::renderWindow – the plugin was built for another version");
    if (!g_renderWorkspaceHook->hook() || !g_addDamageBoxHook->hook() || !g_addDamageRegionHook->hook())
        fail("could not hook Hyprland's rendering");

    static auto preRender = HyprlandAPI::registerCallbackDynamic(PHANDLE, "preRender", [](void*, SCallbackInfo&, std::any data) {
        try {
            if (const auto O = overviewOf(std::any_cast<PHLMONITOR>(data).get()))
                O->onPreRender();
        } catch (const std::bad_any_cast&) {
            for (auto& [_, overview] : g_overviews)
                overview->onPreRender();
        }
    });

    HyprlandAPI::addDispatcherV2(PHANDLE, "koje:overview", ::onOverview);
    // GNOME's dark overview, drawn over the wallpaper as the windows fly in.
    HyprlandAPI::addConfigValue(PHANDLE, "plugin:koje:overview_background", Hyprlang::INT{0xFF1D1D20});
    HyprlandAPI::reloadConfig();

    return {"koje", "koje's overview: the real windows, scaled", "koje", "1"};
}

APICALL EXPORT void PLUGIN_EXIT() {
    g_pHyprRenderer->m_renderPass.removeAllOfType("CKojeOverviewPassElement");
    g_overviews.clear();
}
