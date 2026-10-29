# Draft: GNOME APIs koje would need (from the GNOME experiment)

Status: collecting, not posted. Every GNOME internal the koje extension
(`gnome/extension/koje@koje.io`) patches or reads is listed here with the
public API that would replace it (ADR 0059). Update this file in the same change
that adds the internal.

| Ask | Why koje needs it | Internal used meanwhile | Limitation |
|---|---|---|---|
| `wp_security_context_v1` in Mutter | Tell sandboxed clients (one per context) from the host's. Withhold privileged protocols. Attach an app id/instance to the connection. | none: contexts use Mutter's socket | L1 |
| Window → app hook in `Shell.WindowTracker` (or a per-window sandboxed app id from a trusted client) | The same app in two contexts should be two apps in the dash and Alt+Tab | `AppIcon.activate` patched to prefer the current workspace | L3, L5 |
| App grid / search filter or app provider | Show the context's apps on its workspace | `AppDisplay`, `AppSearchProvider`, `Dash` internals | L4 |
| Named, persistent workspaces | A context owns a workspace that survives being empty | `WorkspaceTracker._checkWorkspaces` | L6 |
| Initial placement of a window by a trusted client | Restore a state's geometry without a visible move | move on `window-created` | L7 |
| Per-window "suspended" state (no ping, no dialog, no disconnect) | Freezing a context (also useful for Flatpak) | close dialog creation patched | L8 |
| Notification source hint | Mark a context's notifications | `MessageTray` internals | L10 |
