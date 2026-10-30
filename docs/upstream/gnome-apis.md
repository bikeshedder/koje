# Draft: GNOME APIs koje would need (from the GNOME experiment)

Status: collecting, not posted. Every GNOME internal the koje extension
(`gnome/src/extension`) patches or reads is listed here with the
public API that would replace it (ADR 0059). Update this file in the same change
that adds the internal.

| Ask | Why koje needs it | Internal used meanwhile | Limitation |
|---|---|---|---|
| `wp_security_context_v1` in Mutter | Tell sandboxed clients (one per context) from the host's. Withhold privileged protocols. Attach an app id/instance to the connection. | none: contexts use Mutter's socket | L1 |
| Window → app hook in `Shell.WindowTracker` (or a per-window sandboxed app id from a trusted client) | The same app in two contexts should be two apps in the dash and Alt+Tab | `Shell.App.prototype.activate`/`open_new_window` patched: windows on the current workspace first, then the context's program | L3, L5 |
| App grid / search filter or app provider | Show the context's apps on its workspace | `Shell.AppSystem`'s `get_installed`, `lookup_app` and `search` replaced for JS callers; `Shell.App`s from in-memory entries with a JS `get_id`; `installed-changed` emitted to redisplay; `AppDisplay.prototype._loadApps` sees no `folder-children` on a context's workspace (its `_folderSettings` proxied while it runs) | L4 |
| Badge or emblem on an app's icon | Tell a context's program from the host's app of the same name in search, grid and dash | the in-memory `Shell.App`s' `create_icon_texture` returns the icon with a label above it | L4 |
| Dash content provider (favorites and running apps per owner) and per-window app grouping | A context's dash holds its own programs with only its windows; the host's only host apps (ADR 0062) | `Dash.prototype._redisplay` wrapped: its `_appSystem.get_running()` answers per owner; the `AppFavorites` singleton's `getFavoriteMap`, `getFavorites`, `isFavorite`, `addFavorite(AtPos)`, `removeFavorite`, `moveFavoriteToPos` replaced; the `Shell.App`s handed to the dash get JS `state` (own property), `get_windows`, `get_n_windows`, `activate`, `request_quit`, and `notify::state`/`windows-changed` emitted; host apps with context windows get a stand-in `Shell.App` on the same `app_info` | L3 |
| Workspace labels and colors (dots, thumbnails, switcher popup) | Tell a context's workspace from a host one at a glance | the Activities button's dots styled; `ThumbnailsBox.addThumbnails` hooked; `MonitorWorkspaceSwitcherPopup.redisplay` wrapped | L6 |
| Named, persistent workspaces | A context owns a workspace that survives being empty | `Main.wm._workspaceTracker._checkWorkspaces` wrapped: kept workspaces get a `_keepAliveId` while it runs; `_queueCheckWorkspaces` | L6 |
| Initial placement of a window by a trusted client | Restore a state's geometry without a visible move | move on `window-created` | L7 |
| Per-window "suspended" state (no ping, no dialog, no disconnect, no input) | Freezing a context (also useful for Flatpak) | `Meta.Window.prototype.activate`/`activate_with_workspace`, `Meta.Workspace.prototype.activate_with_focus` patched; focus moved away on `notify::focus-window`; `CloseDialog.prototype._initDialog` answers "Wait"; a reactive cover above the window actor | L8 |
| Notification source hint | Mark a context's notifications | `MessageTray` internals | L10 |
