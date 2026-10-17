# Open questions

Undecided items. When one is settled, write an ADR (`record-decision` skill) and
remove it here. Plan tasks that resolve a question are noted in brackets.

## Containers and isolation

- **Nested Podman stability** – the setup is settled (ADR 0014); how stable is it in day-to-day use over weeks?
- **Shared image store** – a read-only `additionalimagestores` works (M2.5). Open: who fills and updates it, and what happens to running contexts during an update?
- **D-Bus filter** – settled: notifications and file chooser yes, host `OpenURI` no. Open: screen sharing, calendar, secrets/keyring. *(M2.4)*
- **Same paths** – does koje reliably mount home volume and folders at the host path, and does dragging files into overlay apps work? *(M2.7)*
- **Context `global`** – is it acceptable that overlay apps run unisolated and see all mounted folders?
- **Hyprland permissions** – how to deny screen capture to context programs without breaking wanted screen sharing (video calls)? *(M2.9)*
- **Base image** – one image for all contexts, or a base plus per-context additions? Who builds it, how is it updated? (M2.6: contexts share the pages of common image layers – keep layers common.)

## Memory

- **Default limits** – fixed values or a share of RAM? *(M6.6)*
- **Hard limit** – how long may an orderly shutdown take before a hard stop? Distance to `memory.max` (currently 10 %)?
- **Swap** – limit swap per context, and does it count towards the limit?

## Desktop and layouts

- **Closed windows** – leave all layouts (current plan) or keep a placeholder an identical program can take again?
- **Multiple monitors** – what happens to a layout when a stored monitor is missing? One context workspace per monitor or one for all?
- **Pinned windows** – part of the active layout or floating above all?
- **Matching windows of the same program** – how to map several windows of one program reliably on restore and state switch? *(M5.10)*
- **Paused contexts** – can Hyprland's "not responding" dialog be suppressed for frozen windows? *(M2.8)*
- **Layout switch speed** – is a batched move fluid enough, which animation fits? *(M2.2)*
- **Hyprland versions** – how does koje cope with config/IPC changes between releases?

## Tool and UI

- **Service model** – async (tokio) or threads in `kojed`? *(M4.1)*
- **Client API** – D-Bus or own Unix socket between `koje`/UI and `kojed`? *(M4.1)*
- **Integrations** – declarative TOML enough, or do some need executable hooks? How does `terminal` detect the cwd reliably?
- **Window IDs** – naming rule and stability across restarts.
- **Podman binding** – when to switch from CLI calls to the REST API?
- **UI toolkit** – iced, egui or Slint for the panel? *(M8.3)*
- **Language** – CLI text and messages in English, German or both?
- **License and publishing** – open source? Which license?
