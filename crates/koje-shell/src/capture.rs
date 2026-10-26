//! Window thumbnails for the overview (M8.67): Hyprland's
//! `hyprland_toplevel_export_v1` renders a window into a shared-memory
//! buffer – also one on a hidden workspace, in the stash or of a paused
//! context, as long as it is mapped. A worker thread owns its own Wayland
//! connection (wayland-client is blocking) and sends each thumbnail, scaled
//! down, as an [`Update::Thumbnail`].
//!
//! Hyprland reports a client that captures more than three frames in half a
//! second as a screencast (its `screencast` event); the shell captures only
//! when the overview opens.

use std::fs::File;
use std::io::Read as _;
use std::os::fd::AsFd as _;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use async_channel::Sender;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_buffer, wl_registry, wl_shm, wl_shm_pool};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, WEnum};

use crate::model::Update;

#[allow(
    clippy::all,
    clippy::pedantic,
    missing_docs,
    non_upper_case_globals,
    non_camel_case_types,
    unused_imports
)]
pub mod protocol {
    use wayland_client;
    use wayland_client::protocol::*;

    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/hyprland-toplevel-export-v1.xml");
    }
    use self::__interfaces::*;

    wayland_scanner::generate_client_code!("protocols/hyprland-toplevel-export-v1.xml");
}

use protocol::hyprland_toplevel_export_frame_v1::{
    self as frame, HyprlandToplevelExportFrameV1 as Frame,
};
use protocol::hyprland_toplevel_export_manager_v1::HyprlandToplevelExportManagerV1 as Manager;

/// The largest thumbnail: the overview never shows a window bigger.
pub const THUMB_WIDTH: u32 = 960;
pub const THUMB_HEIGHT: u32 = 720;

/// A captured window as Hyprland hands it over.
#[derive(Debug, Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    /// `wl_shm` format: 0 ARGB8888, 1 XRGB8888, else a DRM fourcc.
    pub format: u32,
    pub y_invert: bool,
    pub data: Vec<u8>,
}

/// A thumbnail: premultiplied BGRA, 4 bytes a pixel, no padding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub data: Arc<Vec<u8>>,
}

const fn fourcc(code: [u8; 4]) -> u32 {
    u32::from_le_bytes(code)
}

/// The formats Hyprland may read windows in (its renderer's preferred read
/// format, which follows the monitor): 8 or 10 bits per channel.
fn pixel(format: u32, px: [u8; 4]) -> Option<[u8; 4]> {
    let word = u32::from_le_bytes(px);
    let ten = |shift: u32| u8::try_from(((word >> shift) & 0x3ff) >> 2).unwrap_or(u8::MAX);
    // Out: B, G, R, A.
    Some(match format {
        0 => px,                         // ARGB8888: B G R A in memory
        1 => [px[0], px[1], px[2], 255], // XRGB8888
        f if f == fourcc(*b"AB24") => [px[2], px[1], px[0], px[3]],
        f if f == fourcc(*b"XB24") => [px[2], px[1], px[0], 255],
        f if f == fourcc(*b"XR30") || f == fourcc(*b"AR30") => [ten(0), ten(10), ten(20), 255],
        f if f == fourcc(*b"XB30") || f == fourcc(*b"AB30") => [ten(20), ten(10), ten(0), 255],
        _ => return None,
    })
}

/// The image scaled down to fit `max_w` × `max_h` (never up), averaging
/// the pixels each thumbnail pixel covers; `None` for a format it does not
/// know or a short buffer.
pub fn thumbnail(img: &Image, max_w: u32, max_h: u32) -> Option<Thumbnail> {
    if img.width == 0 || img.height == 0 {
        return None;
    }
    let need = u64::from(img.stride) * u64::from(img.height);
    if (img.data.len() as u64) < need || img.stride < img.width * 4 {
        return None;
    }
    pixel(img.format, [0; 4])?;
    let scale = (f64::from(max_w) / f64::from(img.width))
        .min(f64::from(max_h) / f64::from(img.height))
        .min(1.0);
    // At most the source size, at least one pixel.
    let size = |v: u32| round_within(f64::from(v) * scale, v);
    let (w, h) = (size(img.width), size(img.height));
    let mut out = Vec::with_capacity(w as usize * h as usize * 4);
    for ty in 0..h {
        let (y0, y1) = span(ty, h, img.height);
        for tx in 0..w {
            let (x0, x1) = span(tx, w, img.width);
            let mut sum = [0u32; 4];
            let mut n = 0u32;
            for sy in y0..y1 {
                let row = if img.y_invert {
                    img.height - 1 - sy
                } else {
                    sy
                };
                let base = row as usize * img.stride as usize;
                for sx in x0..x1 {
                    let at = base + sx as usize * 4;
                    let px = [
                        img.data[at],
                        img.data[at + 1],
                        img.data[at + 2],
                        img.data[at + 3],
                    ];
                    let p = pixel(img.format, px)?;
                    for (s, c) in sum.iter_mut().zip(p) {
                        *s += u32::from(c);
                    }
                    n += 1;
                }
            }
            out.extend(sum.map(|s| u8::try_from(s / n.max(1)).unwrap_or(u8::MAX)));
        }
    }
    Some(Thumbnail {
        width: w,
        height: h,
        data: Arc::new(out),
    })
}

/// `x` rounded, between 1 and `max`.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn round_within(x: f64, max: u32) -> u32 {
    x.round().clamp(1.0, f64::from(max.max(1))) as u32
}

/// The source pixels `i` of `n` covers out of `len`.
fn span(i: u32, n: u32, len: u32) -> (u32, u32) {
    let start = u64::from(i) * u64::from(len) / u64::from(n);
    let end = (u64::from(i) + 1) * u64::from(len) / u64::from(n);
    let start = u32::try_from(start).unwrap_or(len);
    (
        start,
        u32::try_from(end).unwrap_or(len).max(start + 1).min(len),
    )
}

/// Hyprland's handle of a window: the low 32 bits of its address.
pub fn handle(address: &str) -> Option<u32> {
    let value = u64::from_str_radix(address.trim_start_matches("0x"), 16).ok()?;
    Some(u32::try_from(value & 0xffff_ffff).unwrap_or(0))
}

#[derive(Debug, Default)]
struct State {
    /// format, width, height, stride
    buffer: Option<(u32, u32, u32, u32)>,
    y_invert: bool,
    /// `Some(true)` ready, `Some(false)` failed.
    done: Option<bool>,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

macro_rules! ignore_events {
    ($($ty:ty),*) => {$(
        impl Dispatch<$ty, ()> for State {
            fn event(_: &mut Self, _: &$ty, _: <$ty as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}
ignore_events!(
    wl_shm::WlShm,
    wl_shm_pool::WlShmPool,
    wl_buffer::WlBuffer,
    Manager
);

impl Dispatch<Frame, ()> for State {
    fn event(
        state: &mut Self,
        _: &Frame,
        event: frame::Event,
        &(): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            frame::Event::Buffer {
                format,
                width,
                height,
                stride,
            } => {
                let format = match format {
                    WEnum::Value(f) => f as u32,
                    WEnum::Unknown(f) => f,
                };
                state.buffer = Some((format, width, height, stride));
            }
            frame::Event::Flags { flags } => {
                state.y_invert = match flags {
                    WEnum::Value(f) => f.contains(frame::Flags::YInvert),
                    WEnum::Unknown(f) => f & 1 != 0,
                };
            }
            frame::Event::Ready { .. } => state.done = Some(true),
            frame::Event::Failed => state.done = Some(false),
            _ => {}
        }
    }
}

/// A connection to the compositor that captures windows.
pub struct Capturer {
    queue: EventQueue<State>,
    shm: wl_shm::WlShm,
    manager: Manager,
}

impl Capturer {
    pub fn connect() -> Result<Self> {
        let conn = Connection::connect_to_env().context("connecting to the compositor")?;
        let (globals, queue) =
            registry_queue_init::<State>(&conn).context("reading the compositor's globals")?;
        let qh = queue.handle();
        let shm = globals
            .bind::<wl_shm::WlShm, _, _>(&qh, 1..=1, ())
            .context("no wl_shm")?;
        let manager = globals
            .bind::<Manager, _, _>(&qh, 1..=1, ())
            .context("the compositor has no hyprland_toplevel_export_manager_v1")?;
        Ok(Self {
            queue,
            shm,
            manager,
        })
    }

    /// Capture the window at `address`.
    pub fn capture(&mut self, address: &str) -> Result<Image> {
        let handle = handle(address).with_context(|| format!("bad window address {address}"))?;
        let qh = self.queue.handle();
        let mut state = State::default();
        let frame = self.manager.capture_toplevel(0, handle, &qh, ());
        self.queue.roundtrip(&mut state)?;
        let result = self.copy(&frame, &mut state);
        frame.destroy();
        let _ = self.queue.flush();
        result
    }

    fn copy(&mut self, frame: &Frame, state: &mut State) -> Result<Image> {
        if state.done == Some(false) {
            bail!("the compositor cannot capture this window");
        }
        let Some((format, width, height, stride)) = state.buffer else {
            bail!("the compositor offered no buffer");
        };
        let size = u64::from(stride) * u64::from(height);
        let mut file = shm_file(size)?;
        let qh = self.queue.handle();
        let pool = self.shm.create_pool(
            file.as_fd(),
            i32::try_from(size).context("window too large")?,
            &qh,
            (),
        );
        let shm_format = match format {
            0 => WEnum::Value(wl_shm::Format::Argb8888),
            1 => WEnum::Value(wl_shm::Format::Xrgb8888),
            f => wl_shm::Format::try_from(f).map_or(WEnum::Unknown(f), WEnum::Value),
        };
        let WEnum::Value(shm_format) = shm_format else {
            pool.destroy();
            bail!("unknown pixel format {format:#x}");
        };
        let as_i32 = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
        let buffer = pool.create_buffer(
            0,
            as_i32(width),
            as_i32(height),
            as_i32(stride),
            shm_format,
            &qh,
            (),
        );
        frame.copy(&buffer, 1);
        let deadline = Instant::now() + Duration::from_secs(2);
        while state.done.is_none() && Instant::now() < deadline {
            self.queue.roundtrip(state)?;
        }
        buffer.destroy();
        pool.destroy();
        match state.done {
            Some(true) => {}
            Some(false) => bail!("the compositor could not copy the window"),
            None => bail!("the compositor did not answer"),
        }
        let mut data = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
        file.read_to_end(&mut data)?;
        Ok(Image {
            width,
            height,
            stride,
            format,
            y_invert: state.y_invert,
            data,
        })
    }
}

/// An unlinked file in the runtime directory (tmpfs) for a shared buffer.
fn shm_file(size: u64) -> Result<File> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").context("XDG_RUNTIME_DIR is not set")?;
    let path = std::path::Path::new(&dir).join(format!(
        "koje-shell-capture-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    ));
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&path)?;
    std::fs::remove_file(&path)?;
    file.set_len(size)?;
    Ok(file)
}

/// Start the capture thread: it takes window addresses and sends their
/// thumbnails. Without the protocol (another compositor) it logs once and
/// drops the requests.
pub fn spawn(tx: Sender<Update>) -> mpsc::Sender<Vec<String>> {
    let (requests, rx) = mpsc::channel::<Vec<String>>();
    let spawned = std::thread::Builder::new()
        .name("capture".into())
        .spawn(move || {
            let mut capturer = match Capturer::connect() {
                Ok(c) => c,
                Err(e) => {
                    tracing::info!("no window thumbnails: {e:#}");
                    while rx.recv().is_ok() {}
                    return;
                }
            };
            while let Ok(addresses) = rx.recv() {
                for address in addresses {
                    match capturer
                        .capture(&address)
                        .map(|img| thumbnail(&img, THUMB_WIDTH, THUMB_HEIGHT))
                    {
                        Ok(Some(thumb)) => {
                            if tx
                                .send_blocking(Update::Thumbnail { address, thumb })
                                .is_err()
                            {
                                return;
                            }
                        }
                        Ok(None) => tracing::debug!("{address}: pixel format not supported"),
                        Err(e) => tracing::debug!("capturing {address}: {e:#}"),
                    }
                }
            }
        });
    if let Err(e) = spawned {
        tracing::warn!("starting the capture thread: {e}");
    }
    requests
}

/// `koje-shell capture-probe [<dir>]` (the M8.67 spike, on the host): every
/// window Hyprland lists – captured, how long it took, its size – and, with
/// a directory, each thumbnail as a PPM image there.
pub fn probe(dir: Option<&std::path::Path>) -> Result<()> {
    #[derive(serde::Deserialize)]
    struct Client {
        address: String,
        class: String,
        workspace: Workspace,
    }
    #[derive(serde::Deserialize)]
    struct Workspace {
        name: String,
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let reply = runtime.block_on(koje_hyprland::Hyprland::from_env().request("j/clients"))?;
    let clients: Vec<Client> = serde_json::from_str(&reply).context("Hyprland's window list")?;
    let mut capturer = Capturer::connect()?;
    for c in clients {
        let start = Instant::now();
        let outcome = capturer.capture(&c.address);
        let took = start.elapsed();
        let what = format!("{} {} ({})", c.address, c.class, c.workspace.name);
        match outcome {
            Ok(img) => {
                let start = Instant::now();
                let thumb = thumbnail(&img, THUMB_WIDTH, THUMB_HEIGHT);
                let scaled = start.elapsed();
                println!(
                    "ok      {what}: {}x{} format {:#x}{} in {took:.1?}, thumbnail {} in {scaled:.1?}",
                    img.width,
                    img.height,
                    img.format,
                    if img.y_invert { " y-inverted" } else { "" },
                    thumb
                        .as_ref()
                        .map_or("not possible".to_owned(), |t| format!(
                            "{}x{}",
                            t.width, t.height
                        )),
                );
                if let (Some(dir), Some(t)) = (dir, thumb) {
                    let name = format!("{}-{}.ppm", c.address, c.class.replace('/', "_"));
                    std::fs::write(dir.join(name), ppm(&t))?;
                }
            }
            Err(e) => println!("failed  {what}: {e:#} after {took:.1?}"),
        }
    }
    Ok(())
}

/// A thumbnail as a binary PPM (RGB, alpha dropped).
fn ppm(t: &Thumbnail) -> Vec<u8> {
    let mut out = format!("P6\n{} {}\n255\n", t.width, t.height).into_bytes();
    for px in t.data.as_chunks::<4>().0 {
        out.extend([px[2], px[1], px[0]]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(format: u32, w: u32, h: u32, px: [u8; 4]) -> Image {
        let stride = w * 4 + 8;
        let mut data = Vec::new();
        for _ in 0..h {
            for _ in 0..w {
                data.extend(px);
            }
            data.extend([0; 8]);
        }
        Image {
            width: w,
            height: h,
            stride,
            format,
            y_invert: false,
            data,
        }
    }

    #[test]
    fn handles_are_the_low_bits_of_the_address() {
        assert_eq!(handle("0x55d0e8c2a1b0"), Some(0xe8c2_a1b0));
        assert_eq!(handle("2a"), Some(42));
        assert_eq!(handle("0xzz"), None);
    }

    #[test]
    fn scaled_down_keeping_the_proportions_never_up() {
        let t = thumbnail(&image(0, 400, 200, [10, 20, 30, 255]), 100, 100).unwrap();
        assert_eq!((t.width, t.height), (100, 50));
        assert_eq!(t.data.len(), 100 * 50 * 4);
        assert_eq!(&t.data[..4], &[10, 20, 30, 255]);
        let t = thumbnail(&image(0, 40, 20, [1, 2, 3, 4]), 100, 100).unwrap();
        assert_eq!((t.width, t.height), (40, 20));
    }

    #[test]
    fn formats_become_bgra() {
        let one = |format: u32, px: [u8; 4]| {
            thumbnail(&image(format, 1, 1, px), 10, 10).map(|t| t.data[..4].to_vec())
        };
        assert_eq!(one(1, [10, 20, 30, 0]), Some(vec![10, 20, 30, 255]));
        assert_eq!(
            one(fourcc(*b"AB24"), [30, 20, 10, 128]),
            Some(vec![10, 20, 30, 128])
        );
        // XRGB2101010: blue in the low bits.
        let word: u32 = (1023 << 20) | (512 << 10) | 4;
        assert_eq!(
            one(fourcc(*b"XR30"), word.to_le_bytes()),
            Some(vec![1, 128, 255, 255])
        );
        assert_eq!(one(fourcc(*b"NV12"), [0; 4]), None);
    }

    #[test]
    fn upside_down_frames_are_turned() {
        let mut img = image(0, 1, 2, [0, 0, 0, 255]);
        let stride = img.stride as usize;
        img.data[stride..stride + 4].copy_from_slice(&[9, 9, 9, 255]);
        img.y_invert = true;
        let t = thumbnail(&img, 10, 10).unwrap();
        assert_eq!(&t.data[..4], &[9, 9, 9, 255]);
    }

    #[test]
    fn short_buffers_are_refused() {
        let mut img = image(0, 4, 4, [0; 4]);
        img.data.truncate(10);
        assert_eq!(thumbnail(&img, 10, 10), None);
    }
}
