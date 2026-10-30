//! Which Wayland globals Mutter offers (ADR 0059, L1). Contexts share its
//! socket, so `koje doctor` checks that each global is one spike G.2 reviewed.
//! A few raw `wl_display` requests; no Wayland library for one registry read.

use std::collections::BTreeSet;
use std::io::{Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};

/// Globals reviewed in spike G.2 (lab and host with Xwayland): none captures
/// the screen, injects input, lists windows or reads the clipboard in the
/// background.
pub const REVIEWED: &[&str] = &[
    "gtk_shell1",
    "wl_compositor",
    "wl_data_device_manager",
    "wl_drm",
    "wl_eglstream_display",
    "wl_fixes",
    "wl_output",
    "wl_seat",
    "wl_shm",
    "wl_subcompositor",
    "wp_color_manager_v1",
    "wp_color_representation_manager_v1",
    "wp_commit_timing_manager_v1",
    "wp_cursor_shape_manager_v1",
    "wp_drm_lease_device_v1",
    "wp_fifo_manager_v1",
    "wp_fractional_scale_manager_v1",
    "wp_linux_drm_syncobj_manager_v1",
    "wp_pointer_warp_v1",
    "wp_presentation",
    "wp_single_pixel_buffer_manager_v1",
    "wp_viewporter",
    "xdg_activation_v1",
    "xdg_system_bell_v1",
    "xdg_toplevel_drag_manager_v1",
    "xdg_toplevel_tag_manager_v1",
    "xdg_wm_base",
    "xdg_wm_dialog_v1",
    "zwp_idle_inhibit_manager_v1",
    "zwp_keyboard_shortcuts_inhibit_manager_v1",
    "zwp_linux_dmabuf_v1",
    "zwp_pointer_constraints_v1",
    "zwp_pointer_gestures_v1",
    "zwp_primary_selection_device_manager_v1",
    "zwp_relative_pointer_manager_v1",
    "zwp_tablet_manager_v2",
    "zwp_text_input_manager_v3",
    "zxdg_exporter_v1",
    "zxdg_exporter_v2",
    "zxdg_importer_v1",
    "zxdg_importer_v2",
    "zxdg_output_manager_v1",
];

/// The globals in `globals` that G.2 did not review.
pub fn unreviewed(globals: &BTreeSet<String>) -> Vec<&str> {
    globals
        .iter()
        .map(String::as_str)
        .filter(|g| !REVIEWED.contains(g))
        .collect()
}

const DISPLAY: u32 = 1;
const REGISTRY: u32 = 2;
const CALLBACK: u32 = 3;

/// `wl_display.get_registry`, then `wl_display.sync`: the compositor sends
/// every global, then `wl_callback.done`.
fn request() -> Vec<u8> {
    let mut out = Vec::new();
    for (opcode, new_id) in [(1u32, REGISTRY), (0, CALLBACK)] {
        out.extend(DISPLAY.to_ne_bytes());
        out.extend(((12u32 << 16) | opcode).to_ne_bytes());
        out.extend(new_id.to_ne_bytes());
    }
    out
}

fn word(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_ne_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// A Wayland string argument: length with the NUL, bytes, padding.
fn string(b: &[u8]) -> Result<String> {
    let bad = || anyhow!("malformed Wayland string");
    let len = word(b, 0).ok_or_else(bad)? as usize;
    let bytes = b.get(4..4 + len.saturating_sub(1)).ok_or_else(bad)?;
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

/// The interfaces of the `wl_registry.global` events in `buf`, once
/// `wl_callback.done` has come; `None` while more is to be read.
pub fn parse(buf: &[u8]) -> Result<Option<BTreeSet<String>>> {
    let mut globals = BTreeSet::new();
    let mut rest = buf;
    while let (Some(object), Some(head)) = (word(rest, 0), word(rest, 4)) {
        let size = (head >> 16) as usize;
        if size < 8 || !size.is_multiple_of(4) {
            bail!("malformed Wayland message (size {size})");
        }
        let Some(body) = rest.get(8..size) else { break };
        match (object, head & 0xffff) {
            (DISPLAY, 0) => bail!(
                "Wayland error: {}",
                string(body.get(8..).unwrap_or_default())?
            ),
            (REGISTRY, 0) => {
                globals.insert(string(body.get(4..).unwrap_or_default())?);
            }
            (CALLBACK, 0) => return Ok(Some(globals)),
            _ => {}
        }
        rest = rest.get(size..).unwrap_or_default();
    }
    Ok(None)
}

fn socket() -> Result<PathBuf> {
    let display = std::env::var_os("WAYLAND_DISPLAY")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .context("WAYLAND_DISPLAY is not set")?;
    if display.is_absolute() {
        return Ok(display);
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?;
    Ok(PathBuf::from(runtime).join(display))
}

/// The interfaces of the compositor's globals, as any client sees them.
pub fn globals() -> Result<BTreeSet<String>> {
    let path = socket()?;
    let mut stream =
        UnixStream::connect(&path).with_context(|| format!("connecting to {}", path.display()))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(&request())?;
    let mut buf = Vec::new();
    let mut chunk = [0; 4096];
    loop {
        let n = stream
            .read(&mut chunk)
            .context("reading from the compositor")?;
        if n == 0 {
            bail!("the compositor closed the connection");
        }
        buf.extend_from_slice(chunk.get(..n).unwrap_or_default());
        if let Some(globals) = parse(&buf)? {
            return Ok(globals);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(object: u32, opcode: u32, args: &[u8]) -> Vec<u8> {
        let size = u32::try_from(8 + args.len()).unwrap();
        let mut out = object.to_ne_bytes().to_vec();
        out.extend(((size << 16) | opcode).to_ne_bytes());
        out.extend(args);
        out
    }

    fn arg_string(s: &str) -> Vec<u8> {
        let mut out = u32::try_from(s.len() + 1).unwrap().to_ne_bytes().to_vec();
        out.extend(s.as_bytes());
        out.push(0);
        while out.len() % 4 != 0 {
            out.push(0);
        }
        out
    }

    fn global(name: u32, interface: &str, version: u32) -> Vec<u8> {
        let mut args = name.to_ne_bytes().to_vec();
        args.extend(arg_string(interface));
        args.extend(version.to_ne_bytes());
        message(REGISTRY, 0, &args)
    }

    #[test]
    fn reads_globals_until_done() {
        let mut buf = global(1, "wl_compositor", 6);
        buf.extend(global(2, "wl_seat", 10));
        buf.extend(message(DISPLAY, 1, &7u32.to_ne_bytes())); // delete_id
        assert_eq!(parse(&buf).unwrap(), None);
        // Split mid-message: still waiting.
        let done = message(CALLBACK, 0, &0u32.to_ne_bytes());
        let mut partial = buf.clone();
        partial.extend(&done[..6]);
        assert_eq!(parse(&partial).unwrap(), None);
        buf.extend(done);
        let globals = parse(&buf).unwrap().unwrap();
        assert_eq!(
            globals.into_iter().collect::<Vec<_>>(),
            ["wl_compositor", "wl_seat"]
        );
    }

    #[test]
    fn reports_protocol_errors() {
        let mut args = 1u32.to_ne_bytes().to_vec();
        args.extend(1u32.to_ne_bytes());
        args.extend(arg_string("invalid method"));
        let err = parse(&message(DISPLAY, 0, &args)).unwrap_err();
        assert_eq!(err.to_string(), "Wayland error: invalid method");
        // A size smaller than the header.
        let mut short = DISPLAY.to_ne_bytes().to_vec();
        short.extend((4u32 << 16).to_ne_bytes());
        assert!(parse(&short).is_err());
    }

    #[test]
    fn requests_registry_then_sync() {
        let r = request();
        assert_eq!(r.len(), 24);
        assert_eq!(word(&r, 0), Some(DISPLAY));
        assert_eq!(word(&r, 4), Some((12 << 16) | 1));
        assert_eq!(word(&r, 8), Some(REGISTRY));
        assert_eq!(word(&r, 16), Some(12 << 16));
        assert_eq!(word(&r, 20), Some(CALLBACK));
    }

    #[test]
    fn flags_new_globals() {
        let globals: BTreeSet<String> = ["wl_seat", "ext_image_copy_capture_manager_v1"]
            .map(String::from)
            .into();
        assert_eq!(unreviewed(&globals), ["ext_image_copy_capture_manager_v1"]);
        assert!(REVIEWED.is_sorted());
    }
}
