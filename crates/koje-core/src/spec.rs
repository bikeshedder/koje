//! The container spec: everything koje passes to the container runtime when it
//! creates a context container. It collects the rules from the spikes and ADRs
//! in one pure function ([`build`]); host facts come in as [`HostEnv`], and the
//! Podman adapter only turns the spec into arguments.
//!
//! Sources: M2.3 report (`podman run` spec), ADR 0014 (nested Podman), 0015
//! (runtime environment), 0016 (GPU), 0017/0019 (access), 0022 (Wayland).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::access::Access;
use crate::config::Limits;
use crate::container::{LABEL_CONTEXT, container_name, home_volume};
use crate::name::ContextName;
use crate::size::ByteSize;

/// Where the image's (Fedora) GBM looks for backends (ADR 0016).
pub const IMAGE_GBM_DIR: &str = "/usr/lib64/gbm";

/// `/dev/shm` size (ADR 0015): a limit, not reserved memory.
pub const SHM_SIZE: ByteSize = ByteSize(1 << 30);

/// The kernel's `memory.max` lies this many percent above the hard limit, as a
/// safety net if the orderly shutdown does not happen in time (open question).
pub const MEMORY_MAX_HEADROOM_PERCENT: u64 = 10;

/// Facts about the host that the spec depends on (detected by the adapter).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostEnv {
    pub uid: u32,
    /// The user's home; the context's home volume is mounted at this path.
    pub home: PathBuf,
    /// Wayland socket for the context. ADR 0022: kojed's security-context socket;
    /// until kojed exists (M4.10) the compositor's socket.
    pub wayland_socket: Option<PathBuf>,
    pub pipewire_socket: Option<PathBuf>,
    pub pulse_socket: Option<PathBuf>,
    pub cups_socket: Option<PathBuf>,
    /// `/dev/dri` exists.
    pub dri: bool,
    /// An NVIDIA CDI spec exists (ADR 0016).
    pub nvidia_cdi: bool,
    /// The host's NVIDIA GBM backend, resolved (`…/gbm/nvidia-drm_gbm.so` → real file).
    pub nvidia_gbm: Option<PathBuf>,
    /// Per-context directory holding the filtered host bus socket (ADR 0019).
    pub host_bus_dir: Option<PathBuf>,
}

/// How a path inside the container is filled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountKind {
    /// A host file or directory.
    Bind { source: PathBuf, read_only: bool },
    /// A named volume, owned by the user (`:U`).
    Volume { name: String },
    /// A private tmpfs owned by the user.
    Tmpfs { mode: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecMount {
    pub target: PathBuf,
    pub kind: MountKind,
}

/// Everything needed to create a context container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerSpec {
    pub name: String,
    pub hostname: String,
    pub image: String,
    pub labels: BTreeMap<String, String>,
    pub env: BTreeMap<String, String>,
    pub mounts: Vec<SpecMount>,
    /// Device nodes (`/dev/fuse`) and CDI devices (`nvidia.com/gpu=all`).
    pub devices: Vec<String>,
    pub security_opts: Vec<String>,
    /// `false` → no network at all.
    pub network: bool,
    pub shm_size: ByteSize,
    /// The kernel's hard cap (`memory.max`), above koje's own hard limit.
    pub memory_max: Option<ByteSize>,
    pub workdir: PathBuf,
    /// The main process: the context's own session bus (ADR 0015), under `--init`.
    pub command: Vec<String>,
}

/// A spec plus notes about access switches the host could not satisfy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Built {
    pub spec: ContainerSpec,
    pub notes: Vec<String>,
}

/// Inputs from the context's configuration, already resolved.
#[derive(Debug, Clone, Copy)]
pub struct ContextInputs<'a> {
    pub name: &'a ContextName,
    pub image: &'a str,
    /// Effective access (ADR 0017).
    pub access: Access,
    /// Effective limits (context over global defaults).
    pub limits: &'a Limits,
    /// Host folders, expanded, canonicalized and checked (ADR 0009); mounted at
    /// the same path.
    pub mounts: &'a [PathBuf],
}

fn bind(source: &Path, target: impl Into<PathBuf>, read_only: bool) -> SpecMount {
    SpecMount {
        target: target.into(),
        kind: MountKind::Bind {
            source: source.to_owned(),
            read_only,
        },
    }
}

/// The parts of a spec that access switches add to, plus notes.
struct Parts<'h> {
    host: &'h HostEnv,
    run: PathBuf,
    env: BTreeMap<String, String>,
    mounts: Vec<SpecMount>,
    devices: Vec<String>,
    security_opts: Vec<String>,
    notes: Vec<String>,
}

impl Parts<'_> {
    fn set(&mut self, key: &str, value: impl Into<String>) {
        self.env.insert(key.to_owned(), value.into());
    }

    /// The host resource if the switch is on; a note if the host lacks it.
    fn need(&mut self, on: bool, socket: Option<&PathBuf>, what: &str) -> Option<PathBuf> {
        if on && socket.is_none() {
            self.notes
                .push(format!("{what}: not available on the host"));
        }
        socket.filter(|_| on).cloned()
    }

    fn wayland(&mut self, on: bool) {
        let Some(socket) = self.need(on, self.host.wayland_socket.as_ref(), "wayland") else {
            return;
        };
        let file = socket
            .file_name()
            .map_or_else(|| "wayland-0".into(), |n| n.to_string_lossy().into_owned());
        self.mounts.push(bind(&socket, self.run.join(&file), false));
        self.set("WAYLAND_DISPLAY", file);
        // libwebrtc decides Wayland vs. X11 by XDG_SESSION_TYPE; GTK uses the
        // host's file chooser only when told to (M2.10).
        for (k, v) in [
            ("XDG_SESSION_TYPE", "wayland"),
            ("MOZ_ENABLE_WAYLAND", "1"),
            ("GDK_BACKEND", "wayland"),
            ("QT_QPA_PLATFORM", "wayland"),
            ("GTK_USE_PORTAL", "1"),
        ] {
            self.set(k, v);
        }
    }

    fn sound(&mut self, on: bool) {
        let host = self.host;
        if let Some(socket) = self.need(on, host.pipewire_socket.as_ref(), "sound (PipeWire)") {
            self.mounts
                .push(bind(&socket, self.run.join("pipewire-0"), false));
        }
        if let Some(socket) = self.need(on, host.pulse_socket.as_ref(), "sound (PulseAudio)") {
            // Its own path: below a root-owned mount point libpulse refuses it (M2.3).
            let target = self.run.join("pulse-native");
            self.set("PULSE_SERVER", format!("unix:{}", target.display()));
            self.mounts.push(bind(&socket, target, false));
        }
    }

    fn print(&mut self, on: bool) {
        if let Some(socket) = self.need(on, self.host.cups_socket.as_ref(), "print (CUPS)") {
            self.set("CUPS_SERVER", socket.display().to_string());
            self.mounts.push(bind(&socket, &socket, false));
        }
    }

    fn host_bus(&mut self, on: bool) {
        let dir = self.host.host_bus_dir.as_ref();
        if let Some(dir) = self.need(on, dir, "host interfaces (bus proxy)") {
            let target = self.run.join("koje-host");
            self.set(
                "KOJE_HOST_BUS",
                format!("unix:path={}/bus", target.display()),
            );
            self.mounts.push(bind(&dir, target, false));
        }
    }

    fn gpu(&mut self, on: bool) {
        if !on {
            return;
        }
        let host = self.host;
        if host.dri {
            self.devices.push("/dev/dri".to_owned());
        }
        if host.nvidia_cdi {
            self.devices.push("nvidia.com/gpu=all".to_owned());
            match &host.nvidia_gbm {
                Some(gbm) => {
                    let target = Path::new(IMAGE_GBM_DIR).join("nvidia-drm_gbm.so");
                    self.mounts.push(bind(gbm, target, true));
                }
                None => self
                    .notes
                    .push("gpu: NVIDIA GBM backend not found on the host".to_owned()),
            }
        }
        if !host.dri && !host.nvidia_cdi {
            self.notes.push("gpu: no GPU found on the host".to_owned());
        }
    }

    fn nested(&mut self, on: bool) {
        if on {
            self.devices
                .extend(["/dev/fuse".to_owned(), "/dev/net/tun".to_owned()]);
            self.security_opts.push("unmask=/proc/sys".to_owned());
        }
    }
}

/// Build the container spec for a context.
pub fn build(ctx: &ContextInputs<'_>, host: &HostEnv) -> Built {
    let run = PathBuf::from(format!("/run/user/{}", host.uid));
    let bus = format!("unix:path={}/bus", run.display());
    let a = ctx.access;
    let mut p = Parts {
        host,
        run: run.clone(),
        env: BTreeMap::new(),
        mounts: vec![
            SpecMount {
                target: host.home.clone(),
                kind: MountKind::Volume {
                    name: home_volume(ctx.name),
                },
            },
            SpecMount {
                target: run.clone(),
                kind: MountKind::Tmpfs { mode: 0o700 },
            },
        ],
        devices: Vec::new(),
        // SELinux hosts only; no effect elsewhere (M2.3).
        security_opts: vec!["label=disable".to_owned()],
        notes: Vec::new(),
    };
    p.set("HOME", host.home.display().to_string());
    p.set("XDG_RUNTIME_DIR", run.display().to_string());
    p.set("DBUS_SESSION_BUS_ADDRESS", bus.clone());

    p.wayland(a.wayland);
    p.sound(a.sound);
    p.print(a.print);
    p.host_bus(
        a.notifications || a.file_chooser || a.settings || a.inhibit || a.screencast || a.print,
    );
    p.gpu(a.gpu);
    p.nested(a.nested);
    for folder in ctx.mounts {
        p.mounts.push(bind(folder, folder, false));
    }

    let memory_max = ctx
        .limits
        .memory_hard
        .map(|h| ByteSize(h.0 + h.0 / 100 * MEMORY_MAX_HEADROOM_PERCENT));

    Built {
        spec: ContainerSpec {
            name: container_name(ctx.name),
            hostname: ctx.name.to_string(),
            image: ctx.image.to_owned(),
            labels: BTreeMap::from([(LABEL_CONTEXT.to_owned(), ctx.name.to_string())]),
            env: p.env,
            mounts: p.mounts,
            devices: p.devices,
            security_opts: p.security_opts,
            network: a.network,
            shm_size: SHM_SIZE,
            memory_max,
            workdir: host.home.clone(),
            command: vec![
                "dbus-daemon".into(),
                "--session".into(),
                "--nofork".into(),
                "--nopidfile".into(),
                format!("--address={bus}"),
            ],
        },
        notes: p.notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::Profile;

    fn host() -> HostEnv {
        HostEnv {
            uid: 1000,
            home: "/home/mj".into(),
            wayland_socket: Some("/run/user/1000/wayland-1".into()),
            pipewire_socket: Some("/run/user/1000/pipewire-0".into()),
            pulse_socket: Some("/run/user/1000/pulse/native".into()),
            cups_socket: Some("/run/cups/cups.sock".into()),
            dri: true,
            nvidia_cdi: true,
            nvidia_gbm: Some("/usr/lib/x86_64-linux-gnu/libnvidia-allocator.so.595.91.07".into()),
            host_bus_dir: Some("/run/user/1000/koje/kp/host-bus".into()),
        }
    }

    fn kp() -> ContextName {
        ContextName::new("kp").unwrap()
    }

    fn built(access: Access, limits: &Limits, mounts: &[PathBuf], host: &HostEnv) -> Built {
        let name = kp();
        build(
            &ContextInputs {
                name: &name,
                image: "koje/base:latest",
                access,
                limits,
                mounts,
            },
            host,
        )
    }

    fn targets(spec: &ContainerSpec) -> Vec<String> {
        spec.mounts
            .iter()
            .map(|m| m.target.display().to_string())
            .collect()
    }

    #[test]
    fn full_access_matches_the_m2_3_spec() {
        let b = built(
            Profile::Full.access(),
            &Limits::default(),
            &["/home/mj/src/kp".into()],
            &host(),
        );
        let s = &b.spec;
        assert_eq!(b.notes, Vec::<String>::new());
        assert_eq!(s.name, "koje-kp");
        assert_eq!(s.labels["io.koje.context"], "kp");
        assert_eq!(s.env["WAYLAND_DISPLAY"], "wayland-1");
        assert_eq!(s.env["XDG_SESSION_TYPE"], "wayland");
        assert_eq!(s.env["GTK_USE_PORTAL"], "1");
        assert_eq!(s.env["PULSE_SERVER"], "unix:/run/user/1000/pulse-native");
        assert_eq!(s.env["CUPS_SERVER"], "/run/cups/cups.sock");
        assert_eq!(
            s.env["KOJE_HOST_BUS"],
            "unix:path=/run/user/1000/koje-host/bus"
        );
        assert_eq!(
            s.env["DBUS_SESSION_BUS_ADDRESS"],
            "unix:path=/run/user/1000/bus"
        );
        assert_eq!(
            targets(s),
            [
                "/home/mj",
                "/run/user/1000",
                "/run/user/1000/wayland-1",
                "/run/user/1000/pipewire-0",
                "/run/user/1000/pulse-native",
                "/run/cups/cups.sock",
                "/run/user/1000/koje-host",
                "/usr/lib64/gbm/nvidia-drm_gbm.so",
                "/home/mj/src/kp",
            ]
        );
        assert_eq!(
            s.devices,
            [
                "/dev/dri",
                "nvidia.com/gpu=all",
                "/dev/fuse",
                "/dev/net/tun"
            ]
        );
        assert_eq!(s.security_opts, ["label=disable", "unmask=/proc/sys"]);
        assert!(s.network);
        assert_eq!(s.shm_size, ByteSize(1 << 30));
        assert_eq!(s.command[0], "dbus-daemon");
        assert_eq!(s.workdir, PathBuf::from("/home/mj"));
    }

    #[test]
    fn home_volume_and_private_runtime_dir() {
        let s = built(Profile::Isolated.access(), &Limits::default(), &[], &host()).spec;
        assert_eq!(
            s.mounts[0],
            SpecMount {
                target: "/home/mj".into(),
                kind: MountKind::Volume {
                    name: "koje-kp-home".into()
                }
            }
        );
        assert_eq!(s.mounts[1].kind, MountKind::Tmpfs { mode: 0o700 });
    }

    #[test]
    fn isolated_gets_nothing_from_the_host() {
        let b = built(Profile::Isolated.access(), &Limits::default(), &[], &host());
        let s = &b.spec;
        assert_eq!(targets(s), ["/home/mj", "/run/user/1000"]);
        assert_eq!(s.devices, Vec::<String>::new());
        assert_eq!(s.security_opts, ["label=disable"]);
        assert!(!s.network);
        assert!(!s.env.contains_key("WAYLAND_DISPLAY"));
        assert_eq!(b.notes, Vec::<String>::new());
    }

    #[test]
    fn desktop_has_no_gpu() {
        let s = built(Profile::Desktop.access(), &Limits::default(), &[], &host()).spec;
        assert!(
            !s.devices
                .iter()
                .any(|d| d == "/dev/dri" || d.starts_with("nvidia"))
        );
        assert!(!targets(&s).iter().any(|t| t.contains("gbm")));
        assert!(s.devices.contains(&"/dev/fuse".to_owned()));
    }

    #[test]
    fn headless_keeps_nested_network_and_notifications() {
        let b = built(Profile::Headless.access(), &Limits::default(), &[], &host());
        let s = &b.spec;
        assert!(s.network);
        assert!(s.devices.contains(&"/dev/net/tun".to_owned()));
        assert!(s.env.contains_key("KOJE_HOST_BUS"));
        assert!(!s.env.contains_key("WAYLAND_DISPLAY"));
        assert!(!s.env.contains_key("PULSE_SERVER"));
    }

    #[test]
    fn missing_host_resources_become_notes() {
        let bare = HostEnv {
            uid: 1000,
            home: "/home/mj".into(),
            ..HostEnv::default()
        };
        let b = built(Profile::Full.access(), &Limits::default(), &[], &bare);
        assert_eq!(
            b.notes,
            [
                "wayland: not available on the host",
                "sound (PipeWire): not available on the host",
                "sound (PulseAudio): not available on the host",
                "print (CUPS): not available on the host",
                "host interfaces (bus proxy): not available on the host",
                "gpu: no GPU found on the host",
            ]
        );
        let nvidia_without_gbm = HostEnv {
            nvidia_gbm: None,
            ..host()
        };
        let b = built(
            Profile::Full.access(),
            &Limits::default(),
            &[],
            &nvidia_without_gbm,
        );
        assert_eq!(b.notes, ["gpu: NVIDIA GBM backend not found on the host"]);
    }

    #[test]
    fn memory_max_is_above_the_hard_limit() {
        let limits = Limits {
            memory_soft: Some(ByteSize(6 << 30)),
            memory_hard: Some(ByteSize(8 << 30)),
        };
        let s = built(Profile::Full.access(), &limits, &[], &host()).spec;
        assert_eq!(
            s.memory_max,
            Some(ByteSize((8 << 30) + (8 << 30) / 100 * 10))
        );
        let s = built(Profile::Full.access(), &Limits::default(), &[], &host()).spec;
        assert_eq!(s.memory_max, None);
    }
}
