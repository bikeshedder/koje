# Open questions

Undecided items. When one is settled, write an ADR (`record-decision` skill) and
remove it here. Plan tasks that resolve a question are noted in brackets.

## Containers and isolation

- **Nested Podman stability** – the setup is settled (ADR 0014); how stable is it in day-to-day use over weeks?
- **Shared image store** – a read-only `additionalimagestores` works (M2.5). Open: who fills and updates it, and what happens to running contexts during an update?
- **D-Bus filter** – settled in ADR 0019 (forwarder + filtered proxy; notifications, file chooser, settings, inhibit, screen sharing, print). Open: calendar/contacts.
- **Context `global`** – is it acceptable that overlay apps run unisolated and see all mounted folders?
- **Sound without microphone** – can PipeWire's permissions give a context playback but not capture (ADR 0017 has one `sound` switch for both)?
- **Sharing secrets between contexts** – each context has its own keyring (ADR 0019). How could selected secrets be shared with another context on purpose?
- **Files outside mounts from the file chooser** – the host portal returns host paths; mount the document portal into contexts?

## Memory

- **Hard limit** – how long may an orderly shutdown take before a hard stop? Distance to `memory.max` (currently 10 %)?
- **Swap** – limit swap per context, and does it count towards the limit?

## Desktop and layouts

- **Closed windows** – leave all layouts (current plan) or keep a placeholder an identical program can take again?
- **Multiple monitors** – what happens to a layout when a stored monitor is missing? One context workspace per monitor or one for all?
- **Pinned windows** – part of the active layout or floating above all?
- **Layout switch animation** – batched moves are fast (M2.2: 0.1 ms for 10 commands); which animation looks right? *(M5.5)*
- **Hyprland versions** – how does koje cope with config/IPC changes between releases?
- **Sharing one context's screen** – the portal shares a monitor, a window or a region, never a workspace, so sharing a monitor exposes whatever context you switch to. Idea: `koje share <context>` creates a headless output only while sharing (`hyprctl output create headless`), moves the context's workspaces onto it for the user to pick in the portal, keeps the shell's surfaces (bar, OSD, notifications) off it, and `koje share --stop` removes it. Not one headless output per workspace all the time: their windows would never be hidden, so they would keep rendering. Open: how the user sees and works in the shared context (a mirror window in the context color, `wl-mirror`?), keeping kojed's display API and monitors.xml away from the headless output, and whether headless outputs and the portal picker behave on current Hyprland (a host spike first).

## Tool and UI

- **Podman binding** – when to switch from CLI calls to the REST API?
- **More of GNOME's bindings and gestures** – ADR 0043 takes over Super alone and Super+arrows. Which other GNOME bindings (Super+Shift+arrows to another monitor, Super+PageUp/PageDown for workspaces, …) and touchpad gestures (three-finger swipes) follow, and what happens to koje's own bindings on the same keys?
- **Language** – CLI text and messages in English, German or both?
- **License and publishing** – open source? Which license?
- **Mounts outside the home** – ADR 0034 limits `mounts` to folders below the home (live mounts, ADR 0033). If folders elsewhere are needed: make the container's root propagate (OCI `rootfsPropagation`, a spike) or recreate the container on `up` while keeping its changes (`podman commit`)?
