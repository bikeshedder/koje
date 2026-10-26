// koje's overview in Hyprland (ADR 0051): while koje-shell shows its
// overview on a monitor, the windows it lists are drawn there – their own
// buffers, live, scaled into the shell's cards – instead of the workspace.
// The apps are not told: no configure, no resize.
#pragma once

#define WLR_USE_UNSTABLE

#include <hyprland/src/plugins/PluginAPI.hpp>
#include <hyprland/src/desktop/DesktopTypes.hpp>
#include <hyprland/src/render/Framebuffer.hpp>
#include <hyprland/src/helpers/AnimatedVariable.hpp>

#include <map>
#include <string>
#include <vector>

inline HANDLE PHANDLE = nullptr;

// The layer surface of the shell's overview: places are relative to it.
inline const std::string SHELL_NAMESPACE = "koje-overview";

struct SOverviewWindow {
    PHLWINDOWREF window;
    // Logical pixels relative to the monitor.
    CBox from;
    CBox target;
    // The window at full size, then halved until near its card's size:
    // scaled down in steps it stays sharp (Hyprland samples linearly).
    std::vector<UP<CFramebuffer>> chain;
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

  private:
    void                         damage();
    void                         redraw(SOverviewWindow& w);

    std::vector<SOverviewWindow> m_windows;
    PHLANIMVAR<float>            m_progress;
    bool                         m_dirty = true;
};

// Finds Hyprland's renderWindow; false: the plugin cannot work.
bool findRenderWindow();

// By monitor name.
inline std::map<std::string, UP<COverview>> g_overviews;
