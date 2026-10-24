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

/// Where the forwarder is mounted inside every context (ADR 0025).
pub const FORWARDER_IN_CONTEXT: &str = "/usr/local/libexec/koje/koje-forward";

/// How to (re)generate the NVIDIA CDI spec (ADR 0016; needs root).
pub const NVIDIA_CDI_GENERATE: &str = "sudo nvidia-ctk cdi generate --output=/etc/cdi/nvidia.yaml";

/// Directory in the context's `XDG_RUNTIME_DIR` holding its Wayland socket.
pub const WAYLAND_DIR: &str = "koje-wayland";
/// The socket's name in kojed's per-context directory.
pub const WAYLAND_SOCKET: &str = "wayland-0";

/// Directory in the context's `XDG_RUNTIME_DIR` holding its sound sockets.
pub const SOUND_DIR: &str = "koje-sound";
/// Link names in kojed's sound directory (ADR 0029).
pub const PIPEWIRE_LINK: &str = "pipewire-0";
pub const PULSE_LINK: &str = "pulse-native";

/// `/dev/shm` size (ADR 0015): a limit, not reserved memory.
pub const SHM_SIZE: ByteSize = ByteSize(1 << 30);

/// Facts about the host that the spec depends on (detected by the adapter).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostEnv {
    pub uid: u32,
    /// The user's home; the context's home volume is mounted at this path.
    pub home: PathBuf,
    /// The context's Wayland socket: kojed's security-context socket (ADR 0022),
    /// in a per-context directory that is mounted (so a recreated socket shows).
    pub wayland_socket: Option<PathBuf>,
    pub pipewire_socket: Option<PathBuf>,
    pub pulse_socket: Option<PathBuf>,
    /// kojed's per-context directory with hard links to the sound sockets
    /// (ADR 0029); mounted, so a restarted sound server reaches the context.
    pub sound_dir: Option<PathBuf>,
    pub cups_socket: Option<PathBuf>,
    /// `/dev/dri` exists.
    pub dri: bool,
    /// An NVIDIA GPU (PCI vendor 0x10de) or its CDI spec; `None` without NVIDIA.
    pub nvidia: Option<Nvidia>,
    /// Per-context directory holding the filtered host bus socket (ADR 0019).
    pub host_bus_dir: Option<PathBuf>,
    /// The static `koje-forward` binary on the host (ADR 0025).
    pub forwarder: Option<PathBuf>,
    /// The home volume's directory, shared in Podman's namespace for live
    /// mounts (ADR 0033); prepared by the CLI before the container starts.
    /// Without it the home is the named volume (no live mounts).
    pub home_dir: Option<PathBuf>,
}

/// The host's NVIDIA setup (ADR 0016).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nvidia {
    pub cdi: NvidiaCdi,
    /// The host's GBM backend, resolved (`…/gbm/nvidia-drm_gbm.so` → real file).
    pub gbm: Option<PathBuf>,
}

/// State of the NVIDIA CDI spec (`nvidia-ctk cdi generate`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NvidiaCdi {
    Missing,
    Current,
    /// Names libraries of another driver version than the one loaded (driver
    /// updated since the spec was generated).
    Outdated,
}

/// How a path inside the container is filled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountKind {
    /// A host file or directory.
    Bind { source: PathBuf, read_only: bool },
    /// A host directory whose later submounts reach the container (`rslave`:
    /// the home volume's directory, ADR 0033).
    Shared { source: PathBuf },
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
        let file = socket.file_name().map_or_else(
            || WAYLAND_SOCKET.into(),
            |n| n.to_string_lossy().into_owned(),
        );
        let dir = self.run.join(WAYLAND_DIR);
        if let Some(host_dir) = socket.parent() {
            self.mounts.push(bind(host_dir, &dir, false));
        }
        // Absolute: the socket is not directly in XDG_RUNTIME_DIR.
        self.set("WAYLAND_DISPLAY", dir.join(file).display().to_string());
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
        let pipewire = self.need(on, host.pipewire_socket.as_ref(), "sound (PipeWire)");
        let pulse = self.need(on, host.pulse_socket.as_ref(), "sound (PulseAudio)");
        if pipewire.is_none() && pulse.is_none() {
            return;
        }
        let Some(host_dir) = &host.sound_dir else {
            self.notes
                .push("sound: no socket directory from kojed".to_owned());
            return;
        };
        // A directory, not the socket files: kojed re-links replaced sockets.
        let dir = self.run.join(SOUND_DIR);
        self.mounts.push(bind(host_dir, &dir, false));
        if pipewire.is_some() {
            self.set(
                "PIPEWIRE_REMOTE",
                dir.join(PIPEWIRE_LINK).display().to_string(),
            );
        }
        if pulse.is_some() {
            // Not below a root-owned mount point: libpulse refuses that (M2.3).
            self.set(
                "PULSE_SERVER",
                format!("unix:{}", dir.join(PULSE_LINK).display()),
            );
        }
    }

    fn print(&mut self, on: bool) {
        if let Some(socket) = self.need(on, self.host.cups_socket.as_ref(), "print (CUPS)") {
            self.set("CUPS_SERVER", socket.display().to_string());
            self.mounts.push(bind(&socket, &socket, false));
        }
    }

    fn host_bus(&mut self, on: bool) {
        let host = self.host;
        let Some(forwarder) = self.need(
            on,
            host.forwarder.as_ref(),
            "host interfaces (koje-forward not installed – run ./install.sh)",
        ) else {
            return;
        };
        self.mounts
            .push(bind(&forwarder, FORWARDER_IN_CONTEXT, true));
        if let Some(dir) = self.need(
            on,
            host.host_bus_dir.as_ref(),
            "host interfaces (bus proxy)",
        ) {
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
        let mut nvidia_usable = false;
        if let Some(nvidia) = &host.nvidia {
            match nvidia.cdi {
                NvidiaCdi::Missing => self.notes.push(format!(
                    "gpu: NVIDIA GPU without a CDI spec, so this context cannot use it – \
                     run: {NVIDIA_CDI_GENERATE}"
                )),
                NvidiaCdi::Outdated => self.notes.push(format!(
                    "gpu: the NVIDIA CDI spec is for another driver version – regenerate it: \
                     {NVIDIA_CDI_GENERATE}"
                )),
                NvidiaCdi::Current => {}
            }
            // An outdated spec may still work partly; the note says what to do.
            if nvidia.cdi != NvidiaCdi::Missing {
                nvidia_usable = true;
                self.devices.push("nvidia.com/gpu=all".to_owned());
                match &nvidia.gbm {
                    Some(gbm) => {
                        let target = Path::new(IMAGE_GBM_DIR).join("nvidia-drm_gbm.so");
                        self.mounts.push(bind(gbm, target, true));
                    }
                    None => self
                        .notes
                        .push("gpu: NVIDIA GBM backend not found on the host".to_owned()),
                }
            }
        }
        if !host.dri && !nvidia_usable {
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
            // The home: the volume, or its directory shared for live mounts
            // (ADR 0033) – the same data.
            SpecMount {
                target: host.home.clone(),
                kind: match &host.home_dir {
                    Some(dir) => MountKind::Shared {
                        source: dir.clone(),
                    },
                    None => MountKind::Volume {
                        name: home_volume(ctx.name),
                    },
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
    // A UTF-8 locale every glibc has, without language packs in the image
    // (foot: "'C' is not a UTF-8 locale").
    p.set("LANG", "C.UTF-8");

    p.wayland(a.wayland);
    p.sound(a.sound);
    p.print(a.print);
    p.host_bus(
        a.notifications || a.file_chooser || a.settings || a.inhibit || a.screencast || a.print,
    );
    p.gpu(a.gpu);
    p.nested(a.nested);
    // With a shared home, folders below it are mounted live (ADR 0033); only
    // those outside it are mounts of the container.
    let real = if host.home_dir.is_some() {
        crate::live_mounts::real_mounts(ctx.mounts, &host.home)
    } else {
        ctx.mounts.to_vec()
    };
    for folder in &real {
        p.mounts.push(bind(folder, folder, false));
    }

    Built {
        spec: ContainerSpec {
            name: container_name(ctx.name),
            hostname: ctx.name.to_string(),
            image: ctx.image.to_owned(),
            labels: {
                let mut labels = BTreeMap::from([(LABEL_CONTEXT.to_owned(), ctx.name.to_string())]);
                // The real mounts, so koje knows which ones to mount live.
                if host.home_dir.is_some() {
                    labels.insert(
                        crate::live_mounts::LABEL_MOUNTS.to_owned(),
                        crate::live_mounts::encode(&real),
                    );
                }
                labels
            },
            env: p.env,
            mounts: p.mounts,
            devices: p.devices,
            security_opts: p.security_opts,
            network: a.network,
            shm_size: SHM_SIZE,
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
            wayland_socket: Some("/run/user/1000/koje/kp/wayland/wayland-0".into()),
            pipewire_socket: Some("/run/user/1000/pipewire-0".into()),
            pulse_socket: Some("/run/user/1000/pulse/native".into()),
            sound_dir: Some("/run/user/1000/koje/kp/sound".into()),
            cups_socket: Some("/run/cups/cups.sock".into()),
            dri: true,
            nvidia: Some(Nvidia {
                cdi: NvidiaCdi::Current,
                gbm: Some("/usr/lib/x86_64-linux-gnu/libnvidia-allocator.so.595.91.07".into()),
            }),
            host_bus_dir: Some("/run/user/1000/koje/kp/host-bus".into()),
            forwarder: Some("/home/mj/.local/lib/koje/koje-forward".into()),
            home_dir: Some(
                "/home/mj/.local/share/containers/storage/volumes/koje-kp-home/_data".into(),
            ),
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
        assert_eq!(
            crate::live_mounts::decode(&s.labels["io.koje.home-mounts"]),
            Some(vec![]),
            "~/src/kp is mounted live (ADR 0033), not by the container"
        );
        assert_eq!(
            s.env["WAYLAND_DISPLAY"],
            "/run/user/1000/koje-wayland/wayland-0"
        );
        assert_eq!(s.env["XDG_SESSION_TYPE"], "wayland");
        assert_eq!(s.env["GTK_USE_PORTAL"], "1");
        assert_eq!(
            s.env["PULSE_SERVER"],
            "unix:/run/user/1000/koje-sound/pulse-native"
        );
        assert_eq!(
            s.env["PIPEWIRE_REMOTE"],
            "/run/user/1000/koje-sound/pipewire-0"
        );
        assert_eq!(s.env["CUPS_SERVER"], "/run/cups/cups.sock");
        assert_eq!(
            s.env["KOJE_HOST_BUS"],
            "unix:path=/run/user/1000/koje-host/bus"
        );
        assert_eq!(
            s.env["DBUS_SESSION_BUS_ADDRESS"],
            "unix:path=/run/user/1000/bus"
        );
        assert_eq!(s.env["LANG"], "C.UTF-8");
        assert_eq!(
            targets(s),
            [
                "/home/mj",
                "/run/user/1000",
                "/run/user/1000/koje-wayland",
                "/run/user/1000/koje-sound",
                "/run/cups/cups.sock",
                "/usr/local/libexec/koje/koje-forward",
                "/run/user/1000/koje-host",
                "/usr/lib64/gbm/nvidia-drm_gbm.so",
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
                kind: MountKind::Shared {
                    source: "/home/mj/.local/share/containers/storage/volumes/koje-kp-home/_data"
                        .into()
                }
            },
            "the home volume's directory, shared for live mounts"
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
                "host interfaces (koje-forward not installed – run ./install.sh): not available on the host",
                "gpu: no GPU found on the host",
            ]
        );
        let nvidia_without_gbm = HostEnv {
            nvidia: Some(Nvidia {
                cdi: NvidiaCdi::Current,
                gbm: None,
            }),
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
    fn nvidia_hints() {
        let no_cdi = HostEnv {
            nvidia: Some(Nvidia {
                cdi: NvidiaCdi::Missing,
                gbm: None,
            }),
            ..host()
        };
        let b = built(Profile::Full.access(), &Limits::default(), &[], &no_cdi);
        assert_eq!(b.notes.len(), 1, "{:?}", b.notes);
        assert!(
            b.notes[0].contains("without a CDI spec")
                && b.notes[0].contains("nvidia-ctk cdi generate")
        );
        assert!(!b.spec.devices.iter().any(|d| d.starts_with("nvidia")));

        let outdated = HostEnv {
            nvidia: Some(Nvidia {
                cdi: NvidiaCdi::Outdated,
                gbm: Some("/x".into()),
            }),
            ..host()
        };
        let b = built(Profile::Full.access(), &Limits::default(), &[], &outdated);
        assert!(
            b.notes[0].contains("another driver version"),
            "{:?}",
            b.notes
        );
        // Still passed: an outdated spec may work partly; the note says what to do.
        assert!(b.spec.devices.contains(&"nvidia.com/gpu=all".to_owned()));

        // Without the gpu switch no hint at all.
        let b = built(Profile::Desktop.access(), &Limits::default(), &[], &no_cdi);
        assert_eq!(b.notes, Vec::<String>::new());
    }

    #[test]
    fn sound_needs_kojeds_directory() {
        let host = HostEnv {
            sound_dir: None,
            ..host()
        };
        let b = built(Profile::Full.access(), &Limits::default(), &[], &host);
        assert!(!b.spec.env.contains_key("PULSE_SERVER"));
        assert!(b.notes.iter().any(|n| n.contains("sound")), "{:?}", b.notes);
    }

    #[test]
    fn only_folders_outside_the_home_are_container_mounts() {
        let b = built(
            Profile::Full.access(),
            &Limits::default(),
            &["/home/mj/src/kp".into(), "/srv/data".into()],
            &host(),
        );
        let t = targets(&b.spec);
        assert!(t.contains(&"/srv/data".to_owned()));
        assert!(!t.contains(&"/home/mj/src/kp".to_owned()));
        assert_eq!(
            crate::live_mounts::decode(&b.spec.labels["io.koje.home-mounts"]),
            Some(vec![PathBuf::from("/srv/data")])
        );
        // Without a shared home (older callers): all of them, no label.
        let plain = HostEnv {
            home_dir: None,
            ..host()
        };
        let b = built(
            Profile::Full.access(),
            &Limits::default(),
            &["/home/mj/src/kp".into()],
            &plain,
        );
        assert!(targets(&b.spec).contains(&"/home/mj/src/kp".to_owned()));
        assert!(!b.spec.labels.contains_key("io.koje.home-mounts"));
    }
}
