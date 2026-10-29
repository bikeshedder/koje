// Monitors by connector name (`DP-1`), as kojed names them.

/** The connector of a monitor index (its first one when mirrored). */
export function connector(index) {
    const logical = global.backend.get_monitor_manager().get_logical_monitors()
        .find(l => l.get_number() === index);
    return logical?.get_monitors()[0]?.get_connector() ?? `${index}`;
}

/** The monitor index of a connector, -1 if there is none. */
export function indexOf(name) {
    return global.backend.get_monitor_manager().get_monitor_for_connector(name);
}

/** Every monitor: connector, size and the room panels take on each side. */
export function list() {
    const workspace = global.workspace_manager.get_active_workspace();
    const out = [];
    for (let i = 0; i < global.display.get_n_monitors(); i++) {
        const g = global.display.get_monitor_geometry(i);
        const a = workspace.get_work_area_for_monitor(i);
        out.push([
            connector(i), g.width, g.height,
            a.x - g.x, a.y - g.y,
            g.x + g.width - (a.x + a.width), g.y + g.height - (a.y + a.height),
        ]);
    }
    return out;
}
