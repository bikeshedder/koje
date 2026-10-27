// Windows outside their context (ADR 0057). A window is foreign when it
// sits on another context's workspace or a host workspace: its context is
// the `koje-<context>` tag kojed gives it (ADR 0018), and a context's
// workspace is named like the context (ADR 0028). Windows on special
// workspaces (overlays, the stash) and host windows are never foreign.
//
// A foreign window glows – its shadow in the context's color, the first
// color of kojed's border rule – and carries a badge with the context's name
// on its top edge. `plugin:koje:foreign_glow` and `plugin:koje:foreign_badge`
// turn them off.
#include "foreign.hpp"
#include "overview.hpp"

#include <hyprland/src/Compositor.hpp>
#include <hyprland/src/config/ConfigDataValues.hpp>
#include <hyprland/src/desktop/Workspace.hpp>
#include <hyprland/src/desktop/rule/windowRule/WindowRuleApplicator.hpp>
#include <hyprland/src/desktop/state/FocusState.hpp>
#include <hyprland/src/desktop/view/Window.hpp>
#include <hyprland/src/helpers/Monitor.hpp>
#include <hyprland/src/render/OpenGL.hpp>
#include <hyprland/src/render/Renderer.hpp>
#include <hyprland/src/render/decorations/IHyprWindowDecoration.hpp>
#include <hyprland/src/render/pass/RectPassElement.hpp>
#include <hyprland/src/render/pass/TexPassElement.hpp>

#include <cmath>

using origUpdateDecorations = void (*)(void*);
using origMoveToWorkspace   = void (*)(void*, PHLWINDOW, PHLWORKSPACE);
static CFunctionHook* g_updateDecorationsHook = nullptr;
static CFunctionHook* g_moveToWorkspaceHook   = nullptr;

// The glow's strength: focused, unfocused.
constexpr float GLOW_FOCUSED   = 0.9F;
constexpr float GLOW_UNFOCUSED = 0.55F;
// The badge: text size (pt), padding around it, distance from the left edge.
constexpr int    BADGE_TEXT = 10;
constexpr double BADGE_PAD_X = 8, BADGE_PAD_Y = 2, BADGE_LEFT = 14;
// What the badge may reach above the window, for damage.
constexpr double BADGE_ABOVE = 16;

static std::string contextOf(const PHLWINDOW& w) {
    for (const auto& t : w->m_ruleApplicator->m_tagKeeper.getTags()) {
        std::string tag = t;
        if (tag.ends_with('*'))
            tag.pop_back();
        if (tag.starts_with("koje-") && tag.size() > 5)
            return tag.substr(5);
    }
    return {};
}

// The window's context when the window is foreign, else empty.
static std::string foreignContext(const PHLWINDOW& w) {
    if (!w || !w->m_isMapped || !w->m_workspace || w->m_workspace->m_isSpecialWorkspace)
        return {};
    auto ctx = contextOf(w);
    return ctx == w->m_workspace->m_name ? std::string{} : ctx;
}

static CHyprColor contextColor(const PHLWINDOW& w) {
    const auto border = w->m_ruleApplicator->activeBorderColor().valueOr(CGradientValueData{});
    return border.m_colors.empty() ? CHyprColor{0.5, 0.5, 0.5, 1.0} : border.m_colors[0];
}

static bool focused(const PHLWINDOW& w) {
    return w == Desktop::focusState()->window();
}

static void hkUpdateDecorations(void* thisptr) {
    ((origUpdateDecorations)g_updateDecorationsHook->m_original)(thisptr);
    static auto* const* PGLOW = (Hyprlang::INT* const*)HyprlandAPI::getConfigValue(PHANDLE, "plugin:koje:foreign_glow")->getDataStaticPtr();
    const auto          w     = ((Desktop::View::CWindow*)thisptr)->m_self.lock();
    if (!**PGLOW || foreignContext(w).empty())
        return;
    auto color = contextColor(w);
    color.a    = focused(w) ? GLOW_FOCUSED : GLOW_UNFOCUSED;
    *w->m_realShadowColor = color;
}

// Hyprland does not refresh a moved window's decorations: whether it is
// foreign may have changed.
static void hkMoveToWorkspace(void* thisptr, PHLWINDOW window, PHLWORKSPACE workspace) {
    ((origMoveToWorkspace)g_moveToWorkspaceHook->m_original)(thisptr, window, workspace);
    if (window)
        window->updateDecorationValues();
}

static CFunctionHook* hookNamed(const std::string& name, const std::string& demangled, void* to) {
    for (const auto& f : HyprlandAPI::findFunctionsByName(PHANDLE, name)) {
        if (f.demangled.starts_with(demangled))
            return HyprlandAPI::createFunctionHook(PHANDLE, f.address, to);
    }
    return nullptr;
}

bool hookForeign() {
    g_updateDecorationsHook = hookNamed("updateDecorationValues", "Desktop::View::CWindow::updateDecorationValues(", (void*)hkUpdateDecorations);
    g_moveToWorkspaceHook   = hookNamed("moveWindowToWorkspaceSafe", "CCompositor::moveWindowToWorkspaceSafe(", (void*)hkMoveToWorkspace);
    return g_updateDecorationsHook && g_moveToWorkspaceHook && g_updateDecorationsHook->hook() && g_moveToWorkspaceHook->hook();
}

class CBadge : public IHyprWindowDecoration {
  public:
    explicit CBadge(PHLWINDOW window) : IHyprWindowDecoration(window), m_window(window) {}

    SDecorationPositioningInfo getPositioningInfo() override {
        SDecorationPositioningInfo info;
        info.policy         = DECORATION_POSITION_ABSOLUTE;
        info.edges          = DECORATION_EDGE_TOP;
        info.desiredExtents.topLeft = Vector2D{0.0, BADGE_ABOVE};
        return info;
    }

    void onPositioningReply(const SDecorationPositioningReply&) override {}

    void draw(PHLMONITOR monitor, float const& a) override {
        static auto* const* PBADGE = (Hyprlang::INT* const*)HyprlandAPI::getConfigValue(PHANDLE, "plugin:koje:foreign_badge")->getDataStaticPtr();
        const auto          w      = m_window.lock();
        const auto          ctx    = **PBADGE ? foreignContext(w) : std::string{};
        if (ctx.empty() || !monitor) {
            m_last = {};
            return;
        }
        const float scale = monitor->m_scale;
        const auto  bg    = contextColor(w);
        // Light text on dark colors, dark on light ones.
        const bool dark = 0.299 * bg.r + 0.587 * bg.g + 0.114 * bg.b < 0.6;
        if (!m_text || m_textFor != ctx || m_textScale != scale || m_textDark != dark) {
            m_text      = g_pHyprOpenGL->renderText(ctx, dark ? CHyprColor{1, 1, 1, 1} : CHyprColor{0, 0, 0, 0.87}, std::round(BADGE_TEXT * scale), false, "", 0, 700);
            m_textFor   = ctx;
            m_textScale = scale;
            m_textDark  = dark;
        }
        if (!m_text)
            return;

        const Vector2D text = m_text->m_size / scale;
        const double   h    = std::round(text.y + 2 * BADGE_PAD_Y);
        Vector2D       pos  = w->m_realPosition->value() + w->m_floatingOffset;
        if (w->m_workspace && !w->m_pinned)
            pos = pos + w->m_workspace->m_renderOffset->value();
        // On the top edge; inside the window where the edge is under the bar.
        CBox box = {pos.x + BADGE_LEFT, pos.y - std::round(h / 2), std::round(text.x + 2 * BADGE_PAD_X), h};
        if (box.y < monitor->m_position.y + monitor->m_reservedArea.top())
            box.y = pos.y + 6;
        m_last = box;

        CBox local = box.copy().translate(-monitor->m_position).scale(scale).round();
        CRectPassElement::SRectData pill;
        pill.box   = local;
        pill.color = bg;
        pill.color.a *= a * (focused(w) ? 1.F : 0.85F);
        pill.round = std::round(local.h / 2);
        g_pHyprRenderer->m_renderPass.add(makeUnique<CRectPassElement>(pill));

        CTexPassElement::SRenderData label;
        label.tex = m_text;
        label.box = {std::round(local.x + (local.w - m_text->m_size.x) / 2), std::round(local.y + (local.h - m_text->m_size.y) / 2), m_text->m_size.x, m_text->m_size.y};
        label.a   = a;
        g_pHyprRenderer->m_renderPass.add(makeUnique<CTexPassElement>(std::move(label)));
    }

    eDecorationType getDecorationType() override {
        return DECORATION_CUSTOM;
    }

    void updateWindow(PHLWINDOW) override {
        damageEntire();
    }

    void damageEntire() override {
        if (!m_last.empty())
            g_pHyprRenderer->damageBox(m_last);
    }

    eDecorationLayer getDecorationLayer() override {
        return DECORATION_LAYER_OVER;
    }

    uint64_t getDecorationFlags() override {
        return DECORATION_NON_SOLID;
    }

    std::string getDisplayName() override {
        return "koje badge";
    }

  private:
    PHLWINDOWREF m_window;
    CBox         m_last;
    SP<CTexture> m_text;
    std::string  m_textFor;
    float        m_textScale = 0;
    bool         m_textDark  = false;
};

void addBadge(PHLWINDOW window) {
    if (window)
        HyprlandAPI::addWindowDecoration(PHANDLE, window, makeUnique<CBadge>(window));
}
