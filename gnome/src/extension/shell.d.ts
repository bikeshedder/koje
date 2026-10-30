// GNOME Shell modules @girs/gnome-shell has no types for.

// The popup when switching workspaces with the keyboard (switcher.ts
// patches it).
declare module 'resource:///org/gnome/shell/ui/workspaceSwitcherPopup.js' {
    import type St from 'gi://St';

    export class MonitorWorkspaceSwitcherPopup {
        _list: St.Widget;
        redisplay(activeIndex: number, ...args: unknown[]): void;
    }
}
