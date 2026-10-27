// koje's overview in Hyprland (ADR 0051): while koje-shell shows its
// overview on a monitor, the windows it lists are drawn there – their own
// buffers, live, scaled into the shell's cards – instead of the workspace.
// The apps are not told: no configure, no resize.
#pragma once

#define WLR_USE_UNSTABLE

#include <hyprland/src/plugins/PluginAPI.hpp>
#include <hyprland/src/desktop/DesktopTypes.hpp>
#include <hyprland/src/render/Framebuffer.hpp>
#include <hyprland/src/helpers/time/Time.hpp>

#include <functional>
#include <map>
#include <string>
#include <vector>

inline HANDLE PHANDLE = nullptr;

// The layer surface of the shell's overview: places are relative to it.
inline const std::string SHELL_NAMESPACE = "koje-overview";

struct SOverviewWindow {
    PHLWINDOWREF window;
    // Logical pixels: where it flies from, relative to the monitor; its
    // card, relative to the shell's overview surface.
    CBox from;
    CBox target;
    // The window at full size, then halved until near its card's size:
    // scaled down in steps it stays sharp (Hyprland samples linearly).
    std::vector<UP<CFramebuffer>> chain;
};

// The overview's own animation, eased out as GNOME's: it takes
// plugin:koje:overview_duration (ms), not one of Hyprland's animations.
class CProgress {
  public:
    float value() const;
    float goal() const {
        return m_to;
    }
    void warp(float v);
    // From where it is now; `onEnd` once there.
    void animateTo(float to, std::function<void()> onEnd = {});
    // Each frame: true while it moves; runs `onEnd` when it arrived.
    bool tick();

  private:
    float                     m_from = 0.F, m_to = 0.F;
    Time::steady_tp           m_start;
    std::chrono::milliseconds m_duration{0};
    std::function<void()>     m_onEnd;
};

class COverview {
  public:
    explicit COverview(PHLMONITOR monitor);
    ~COverview();

    // `animate`: fly in from where the windows are.
    void setWindows(const std::vector<std::pair<PHLWINDOW, CBox>>& targets, bool animate);
    void close();

    void render();
    void draw();
    void onDamage();
    void onPreRender();

    PHLMONITORREF monitor;
    bool          closing     = false;
    bool          blockDamage = false;
    // An overlay shows: drawn after Hyprland darkened and blurred behind it.
    bool          deferred    = false;

  private:
    void                         damage();
    void                         redraw(SOverviewWindow& w);

    void                         fly();

    std::vector<SOverviewWindow> m_windows;
    CProgress                    m_progress;
    bool                         m_dirty = true;
    // The shell's overview surface, relative to the monitor: where the
    // cards are. Read every frame – it is mapped only after `open`.
    Vector2D                     m_origin;
    // Opened, the windows wait for that surface before they fly in.
    bool                         m_waiting = false;
    Time::steady_tp              m_openedAt;
};

// Finds Hyprland's renderWindow; false: the plugin cannot work.
bool findRenderWindow();

// By monitor name.
inline std::map<std::string, UP<COverview>> g_overviews;
