#include "overview.hpp"
#include "pass.hpp"

#include <hyprland/src/Compositor.hpp>
#include <hyprland/src/config/ConfigManager.hpp>
#include <hyprland/src/desktop/view/Window.hpp>
#include <hyprland/src/helpers/Monitor.hpp>
#include <hyprland/src/helpers/time/Time.hpp>
#include <hyprland/src/managers/animation/AnimationManager.hpp>
#include <hyprland/src/managers/eventLoop/EventLoopManager.hpp>
#include <hyprland/src/render/OpenGL.hpp>
#include <hyprland/src/render/Renderer.hpp>

#include <drm_fourcc.h>

// CHyprRenderer::renderWindow is private: called through its address.
using RenderWindowFn = void (*)(void*, PHLWINDOW, PHLMONITOR, const Time::steady_tp&, bool, eRenderPassMode, bool, bool);
static RenderWindowFn g_renderWindow = nullptr;

bool findRenderWindow() {
    for (const auto& f : HyprlandAPI::findFunctionsByName(PHANDLE, "renderWindow")) {
        if (f.demangled.starts_with("CHyprRenderer::renderWindow(")) {
            g_renderWindow = (RenderWindowFn)f.address;
            return true;
        }
    }
    return false;
}

// As Hyprland's own window capture does: at 0,0, standalone, undecorated.
static void renderWindow(PHLWINDOW w, PHLMONITOR m) {
    if (g_renderWindow)
        g_renderWindow(g_pHyprRenderer.get(), w, m, Time::steadyNow(), false, RENDER_PASS_ALL, true, true);
}

static CBox lerp(const CBox& a, const CBox& b, float t) {
    return CBox{a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.w + (b.w - a.w) * t, a.h + (b.h - a.h) * t};
}

// Where a window is now, relative to its monitor.
static CBox placeOf(PHLWINDOW w, PHLMONITOR m) {
    return CBox{w->m_realPosition->value() - m->m_position, w->m_realSize->value()};
}

COverview::COverview(PHLMONITOR monitor_) : monitor(monitor_) {
    g_pAnimationManager->createAnimation(0.F, m_progress, g_pConfigManager->getAnimationPropertyConfig("windowsMove"), AVARDAMAGE_NONE);
    m_progress->setUpdateCallback([this](auto) { damage(); });
}

COverview::~COverview() {
    g_pHyprRenderer->makeEGLCurrent();
    m_windows.clear();
    if (const auto M = monitor.lock())
        g_pHyprRenderer->damageMonitor(M);
}

void COverview::setWindows(const std::vector<std::pair<PHLWINDOW, CBox>>& targets, bool animate) {
    const auto M = monitor.lock();
    if (!M)
        return;
    g_pHyprRenderer->makeEGLCurrent();
    std::vector<SOverviewWindow> next;
    for (const auto& [window, target] : targets) {
        SOverviewWindow w;
        w.window = window;
        w.target = target;
        // Only windows in view fly in; others (another workspace, the
        // stash) appear in their card.
        const bool inView = window->m_workspace && window->m_workspace == M->m_activeWorkspace;
        w.from            = animate && inView ? placeOf(window, M) : target;
        // Keep what is drawn already.
        for (auto& old : m_windows) {
            if (old.window == w.window) {
                w.chain = std::move(old.chain);
                if (!animate)
                    w.from = old.from;
            }
        }
        next.push_back(std::move(w));
    }
    m_windows = std::move(next);
    if (animate) {
        m_progress->setValueAndWarp(0.F);
        *m_progress = 1.F;
    } else if (m_progress->goal() < 1.F && !closing) {
        *m_progress = 1.F;
    }
    m_dirty = true;
    damage();
}

void COverview::close() {
    if (closing)
        return;
    closing      = true;
    const auto M = monitor.lock();
    const auto NAME = M ? M->m_name : std::string{};
    const auto end  = [NAME](auto) {
        g_pEventLoopManager->doLater([NAME] { g_overviews.erase(NAME); });
    };
    // Back into their places – unless the workspace changed meanwhile: then
    // they are not where they would fly to.
    const bool sameWorkspace = M && std::ranges::all_of(m_windows, [&](const auto& w) {
                                   const auto W = w.window.lock();
                                   return W && W->m_workspace == M->m_activeWorkspace;
                               });
    if (!sameWorkspace || m_windows.empty()) {
        end(nullptr);
        return;
    }
    for (auto& w : m_windows) {
        if (const auto W = w.window.lock())
            w.from = placeOf(W, M);
    }
    // Started first: Hyprutils runs an end callback at once for a variable
    // that is not animating.
    *m_progress = 0.F;
    m_progress->setCallbackOnEnd(end);
}

void COverview::render() {
    g_pHyprRenderer->m_renderPass.add(makeUnique<CKojeOverviewPassElement>(this));
}

void COverview::damage() {
    const auto M = monitor.lock();
    if (!M)
        return;
    blockDamage = true;
    g_pHyprRenderer->damageMonitor(M);
    blockDamage = false;
}

// Something on the monitor changed (a window drew): draw them again.
void COverview::onDamage() {
    m_dirty = true;
    damage();
    if (const auto M = monitor.lock())
        g_pCompositor->scheduleFrameForMonitor(M);
}

void COverview::onPreRender() {
    if (!m_dirty)
        return;
    m_dirty = false;
    for (auto& w : m_windows)
        redraw(w);
}

void COverview::redraw(SOverviewWindow& w) {
    const auto M = monitor.lock();
    const auto W = w.window.lock();
    if (!M || !W || !W->m_isMapped)
        return;
    const auto     scale = M->m_scale;
    const Vector2D full  = (W->m_realSize->value() * scale).round();
    if (full.x < 1 || full.y < 1)
        return;
    // Halve until the next half would be smaller than the card.
    std::vector<Vector2D> sizes{full};
    const double          cardW = std::max(1.0, w.target.w * scale);
    while (sizes.back().x / 2 >= cardW && sizes.back().y / 2 >= 1)
        sizes.push_back((sizes.back() / 2).round());
    if (w.chain.size() != sizes.size() || w.chain[0]->m_size != full) {
        w.chain.clear();
        for (const auto& s : sizes) {
            auto fb = makeUnique<CFramebuffer>();
            // With alpha: rounded corners and shadows of the app stay see-through.
            fb->alloc(s.x, s.y, DRM_FORMAT_ABGR8888);
            w.chain.push_back(std::move(fb));
        }
    }

    blockDamage = true;
    CRegion fake{0, 0, INT16_MAX, INT16_MAX};
    if (g_pHyprRenderer->beginRender(M, fake, RENDER_MODE_FULL_FAKE, nullptr, w.chain[0].get())) {
        g_pHyprOpenGL->clear(CHyprColor{0, 0, 0, 0});
        // As Hyprland's own window capture does: at 0,0, the app not told.
        g_pHyprRenderer->m_bBlockSurfaceFeedback = g_pHyprRenderer->shouldRenderWindow(W);
        renderWindow(W, M);
        g_pHyprRenderer->m_bBlockSurfaceFeedback = false;
        g_pHyprOpenGL->m_renderData.blockScreenShader = true;
        g_pHyprRenderer->endRender();
    }
    for (size_t i = 1; i < w.chain.size(); ++i) {
        if (!g_pHyprRenderer->beginRender(M, fake, RENDER_MODE_FULL_FAKE, nullptr, w.chain[i].get()))
            break;
        g_pHyprOpenGL->clear(CHyprColor{0, 0, 0, 0});
        g_pHyprOpenGL->renderTexture(w.chain[i - 1]->getTexture(), CBox{{0, 0}, w.chain[i]->m_size}, {.damage = &fake, .a = 1.F});
        g_pHyprOpenGL->m_renderData.blockScreenShader = true;
        g_pHyprRenderer->endRender();
    }
    blockDamage = false;
}

void COverview::draw() {
    const auto M = monitor.lock();
    if (!M)
        return;
    static auto* const* PBG = (Hyprlang::INT* const*)HyprlandAPI::getConfigValue(PHANDLE, "plugin:koje:overview_background")->getDataStaticPtr();
    const float         t   = m_progress->value();
    CRegion             all{0, 0, INT16_MAX, INT16_MAX};
    CHyprColor          bg{(uint64_t)**PBG};
    bg.a *= t;
    g_pHyprOpenGL->renderRect(CBox{{0, 0}, M->m_pixelSize}, bg, {.damage = &all});
    // In Hyprland's stacking order, bottom first (read every frame: the
    // window clicked is raised and flies back on top).
    std::vector<std::pair<size_t, SOverviewWindow*>> stacked;
    for (auto& w : m_windows) {
        const auto W  = w.window.lock();
        const auto it = std::ranges::find(g_pCompositor->m_windows, W);
        stacked.emplace_back(it - g_pCompositor->m_windows.begin(), &w);
    }
    std::ranges::stable_sort(stacked, {}, &std::pair<size_t, SOverviewWindow*>::first);
    for (auto& [_, wp] : stacked) {
        auto&      w = *wp;
        const auto W = w.window.lock();
        if (w.chain.empty() || !W)
            continue;
        CBox box = lerp(w.from, w.target, t);
        box.scale(M->m_scale).round();
        // As Hyprland draws the window: its rounding, then its border (the
        // context's color) – where it lands, it matches the real one.
        const float rounding = W->rounding() * M->m_scale;
        const float power    = W->roundingPower();
        g_pHyprOpenGL->renderTexture(w.chain.back()->getTexture(), box, {.damage = &all, .a = 1.F, .round = (int)std::round(rounding), .roundingPower = power});
        const int border = W->getRealBorderSize();
        if (border > 0) {
            const float correction = border * (M_SQRT2 - 1) * std::max(2.0 - power, 0.0);
            g_pHyprOpenGL->renderBorder(box, W->m_realBorderColor,
                                        {.round         = (int)std::round(rounding),
                                         .roundingPower = power,
                                         .borderSize    = border,
                                         .a             = 1.F,
                                         .outerRound    = (int)std::round((W->rounding() + border - correction) * M->m_scale)});
        }
    }
}
