// Windows move by their title bars (M8.95). An app that draws its own title
// bar (GTK, libadwaita, Chromium, …) asks the compositor to move the window
// when it is dragged there (`xdg_toplevel.move`); Hyprland 0.53 ignores the
// request – its xdg-shell sets no handler. The plugin sets one and starts
// Hyprland's own drag, as Super+drag does: the window follows the pointer,
// a maximized one leaves its maximized state, and the drag ends when the
// button is released.
//
// The request handler lives on the toplevel's protocol object, which
// Hyprland keeps private: XDGShell.hpp is read with `private` made public,
// in this file only.
#include "drag.hpp"
#include "overview.hpp"

#include <hyprland/src/Compositor.hpp>
#include <hyprland/src/desktop/view/Window.hpp>
#include <hyprland/src/managers/KeybindManager.hpp>
#include <hyprland/src/managers/SeatManager.hpp>
#include <hyprland/src/managers/input/InputManager.hpp>

#define private public
#include <hyprland/src/protocols/XDGShell.hpp>
#undef private

#include <set>

// Pointer buttons held now (Hyprland's own list is private).
static std::set<uint32_t> g_held;

void onMouseButton(const IPointer::SButtonEvent& e) {
    if (e.state == WL_POINTER_BUTTON_STATE_PRESSED)
        g_held.insert(e.button);
    else
        g_held.erase(e.button);
}

static void beginMove(const PHLWINDOWREF& ref) {
    const auto window = ref.lock();
    // Only while a button is held on this window: Hyprland's drag takes the
    // window under the pointer and ends at the next button event.
    if (!window || !window->m_isMapped || g_held.empty() || !g_pInputManager->m_currentlyDraggedWindow.expired())
        return;
    const auto under = g_pCompositor->vectorToWindowUnified(g_pInputManager->getMouseCoordsInternal(),
                                                            Desktop::View::RESERVED_EXTENTS | Desktop::View::INPUT_EXTENTS | Desktop::View::ALLOW_FLOATING);
    if (under != window)
        return;
    // The app lets go of the pointer, as with Mutter: it does not see the
    // release, Hyprland's drag takes it.
    g_pSeatManager->setPointerFocus(nullptr, {});
    CKeybindManager::changeMouseBindMode(MBIND_MOVE);
}

void letMove(PHLWINDOW window) {
    if (!window || !window->m_xdgSurface || !window->m_xdgSurface->m_toplevel || !window->m_xdgSurface->m_toplevel->m_resource)
        return;
    window->m_xdgSurface->m_toplevel->m_resource->setMove([ref = PHLWINDOWREF{window}](CXdgToplevel*, wl_resource*, uint32_t) { beginMove(ref); });
}
