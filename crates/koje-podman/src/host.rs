//! Detect the host facts a container spec needs ([`koje_core::spec::HostEnv`]):
//! which sockets, GPUs and NVIDIA files exist. All reads go through a root
//! directory and an environment lookup, so tests use a fake host.

use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};
use std::path::{Path, PathBuf};

use koje_core::config::ContextConfig;
use koje_core::mount::{HostDirs, check_resolved};
use koje_core::spec::{HostEnv, Nvidia, NvidiaCdi};

/// Where distributions keep NVIDIA's GBM backend (ADR 0016).
const NVIDIA_GBM: &[&str] = &[
    "usr/lib/x86_64-linux-gnu/gbm/nvidia-drm_gbm.so",
    "usr/lib64/gbm/nvidia-drm_gbm.so",
    "usr/lib/gbm/nvidia-drm_gbm.so",
];

/// Where `nvidia-ctk cdi generate` writes its spec (ADR 0016).
const NVIDIA_CDI: &[&str] = &[
    "etc/cdi/nvidia.yaml",
    "var/run/cdi/nvidia.yaml",
    "run/cdi/nvidia.yaml",
];

/// The real host.
pub fn detect() -> std::io::Result<HostEnv> {
    let uid = std::fs::metadata("/proc/self")?.uid();
    Ok(detect_with(uid, Path::new("/"), |k| std::env::var(k).ok()))
}

/// Detect against `root` (normally `/`) with `var` for environment variables.
pub fn detect_with(uid: u32, root: &Path, var: impl Fn(&str) -> Option<String>) -> HostEnv {
    let at = |p: &str| root.join(p.trim_start_matches('/'));
    let socket = |p: PathBuf| is_socket(&p).then_some(p);
    let runtime = var("XDG_RUNTIME_DIR").map_or_else(|| at(&format!("run/user/{uid}")), |d| at(&d));
    let wayland = var("WAYLAND_DISPLAY").and_then(|w| {
        let p = Path::new(&w);
        socket(if p.is_absolute() {
            at(&w)
        } else {
            runtime.join(p)
        })
    });
    HostEnv {
        uid,
        home: var("HOME").map(PathBuf::from).unwrap_or_default(),
        wayland_socket: wayland,
        pipewire_socket: socket(runtime.join("pipewire-0")),
        pulse_socket: socket(runtime.join("pulse/native")),
        cups_socket: socket(at("run/cups/cups.sock")),
        dri: at("dev/dri").is_dir(),
        nvidia: nvidia(root),
        // Started per context by koje (M3.14); not part of host detection.
        host_bus_dir: None,
    }
}

/// The user the base image is built for: name and home from `/etc/passwd`
/// for the current uid (fallback `$USER`/`$HOME`).
pub fn image_user() -> std::io::Result<crate::image::ImageUser> {
    let uid = std::fs::metadata("/proc/self")?.uid();
    let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
    Ok(image_user_from(uid, &passwd, |k| std::env::var(k).ok()))
}

fn image_user_from(
    uid: u32,
    passwd: &str,
    var: impl Fn(&str) -> Option<String>,
) -> crate::image::ImageUser {
    let entry = passwd.lines().find_map(|l| {
        let f: Vec<&str> = l.split(':').collect();
        (f.len() >= 6 && f[2] == uid.to_string()).then(|| (f[0].to_owned(), PathBuf::from(f[5])))
    });
    let (name, home) = entry.unwrap_or_else(|| {
        (
            var("USER").unwrap_or_else(|| format!("user{uid}")),
            var("HOME").map(PathBuf::from).unwrap_or_default(),
        )
    });
    crate::image::ImageUser { name, uid, home }
}

/// A context's mounts ready for the spec: expanded (ADR 0009 rules), resolved
/// to their real paths and checked again, so a symlink cannot lead into a
/// protected folder. Folders must exist.
pub fn resolve_mounts(cfg: &ContextConfig, dirs: &HostDirs) -> koje_core::Result<Vec<PathBuf>> {
    let expanded = cfg.expand_mounts(dirs)?;
    cfg.mounts
        .iter()
        .zip(expanded)
        .map(|(mount, path)| {
            let real = std::fs::canonicalize(&path).map_err(|source| koje_core::Error::Read {
                path: path.clone(),
                source,
            })?;
            check_resolved(mount, &real, dirs)?;
            Ok(real)
        })
        .collect()
}

/// The host's NVIDIA setup: a GPU (or at least a CDI spec), the spec's state
/// compared with the loaded driver, and the GBM backend.
fn nvidia(root: &Path) -> Option<Nvidia> {
    let at = |p: &str| root.join(p);
    let spec = NVIDIA_CDI
        .iter()
        .find_map(|p| std::fs::read_to_string(at(p)).ok());
    if spec.is_none() && !nvidia_gpu(&at("sys/class/drm")) {
        return None;
    }
    let cdi = match (
        spec,
        nvidia_driver_version(&at("proc/driver/nvidia/version")),
    ) {
        (None, _) => NvidiaCdi::Missing,
        (Some(spec), Some(version)) if !spec.contains(&version) => NvidiaCdi::Outdated,
        (Some(_), _) => NvidiaCdi::Current,
    };
    let gbm = NVIDIA_GBM
        .iter()
        .map(|p| at(p))
        .find(|p| p.exists())
        .and_then(|p| std::fs::canonicalize(p).ok());
    Some(Nvidia { cdi, gbm })
}

/// Any DRM card with NVIDIA's PCI vendor id.
fn nvidia_gpu(drm: &Path) -> bool {
    std::fs::read_dir(drm).is_ok_and(|entries| {
        entries.flatten().any(|e| {
            std::fs::read_to_string(e.path().join("device/vendor"))
                .is_ok_and(|v| v.trim() == "0x10de")
        })
    })
}

/// The loaded NVIDIA kernel driver's version, e.g. `595.91.07`, from
/// `/proc/driver/nvidia/version` ("NVRM version: … Kernel Module … 595.91.07 …").
fn nvidia_driver_version(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let line = text.lines().find(|l| l.starts_with("NVRM version"))?;
    line.split_whitespace()
        .find(|w| {
            w.split('.').count() >= 2
                && w.split('.')
                    .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        })
        .map(String::from)
}

fn is_socket(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.file_type().is_socket())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::os::unix::net::UnixListener;

    use super::*;

    #[test]
    fn detects_sockets_gpus_and_nvidia_files() {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path();
        for d in [
            "run/user/1000/pulse",
            "run/cups",
            "dev/dri",
            "etc/cdi",
            "usr/lib/x86_64-linux-gnu/gbm",
        ] {
            std::fs::create_dir_all(r.join(d)).unwrap();
        }
        let _sockets: Vec<UnixListener> = [
            "run/user/1000/wayland-1",
            "run/user/1000/pipewire-0",
            "run/user/1000/pulse/native",
            "run/cups/cups.sock",
        ]
        .iter()
        .map(|s| UnixListener::bind(r.join(s)).unwrap())
        .collect();
        std::fs::write(
            r.join("etc/cdi/nvidia.yaml"),
            "hostPath: /usr/lib/x86_64-linux-gnu/libnvidia-ml.so.595.91.07\n",
        )
        .unwrap();
        for d in [
            "sys/class/drm/card1/device",
            "sys/class/drm/card2/device",
            "proc/driver/nvidia",
        ] {
            std::fs::create_dir_all(r.join(d)).unwrap();
        }
        std::fs::write(r.join("sys/class/drm/card1/device/vendor"), "0x10de\n").unwrap();
        std::fs::write(r.join("sys/class/drm/card2/device/vendor"), "0x1002\n").unwrap();
        std::fs::write(
            r.join("proc/driver/nvidia/version"),
            "NVRM version: NVIDIA UNIX Open Kernel Module for x86_64  595.91.07  Release Build  (dvs-builder@U22)  Tue Jul 29 2026\nGCC version:  gcc version 15.2.0\n",
        )
        .unwrap();
        std::fs::write(
            r.join("usr/lib/x86_64-linux-gnu/libnvidia-allocator.so.595"),
            "",
        )
        .unwrap();
        std::os::unix::fs::symlink(
            "../libnvidia-allocator.so.595",
            r.join("usr/lib/x86_64-linux-gnu/gbm/nvidia-drm_gbm.so"),
        )
        .unwrap();
        let vars = HashMap::from([
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
            ("WAYLAND_DISPLAY", "wayland-1"),
            ("HOME", "/home/mj"),
        ]);
        let h = detect_with(1000, r, |k| vars.get(k).map(|v| (*v).to_owned()));
        assert_eq!(h.home, PathBuf::from("/home/mj"));
        assert_eq!(h.wayland_socket, Some(r.join("run/user/1000/wayland-1")));
        assert_eq!(h.pipewire_socket, Some(r.join("run/user/1000/pipewire-0")));
        assert_eq!(h.pulse_socket, Some(r.join("run/user/1000/pulse/native")));
        assert_eq!(h.cups_socket, Some(r.join("run/cups/cups.sock")));
        assert!(h.dri);
        let nvidia = h.nvidia.clone().unwrap();
        assert_eq!(
            nvidia.cdi,
            NvidiaCdi::Current,
            "spec matches driver 595.91.07"
        );
        // After a driver update the spec still names the old libraries.
        std::fs::write(
            r.join("proc/driver/nvidia/version"),
            "NVRM version: NVIDIA UNIX Open Kernel Module for x86_64  600.10  Release Build\n",
        )
        .unwrap();
        let h2 = detect_with(1000, r, |k| vars.get(k).map(|v| (*v).to_owned()));
        assert_eq!(h2.nvidia.unwrap().cdi, NvidiaCdi::Outdated);
        // The GBM backend link is resolved to the real file (mounted read-only).
        let gbm = nvidia.gbm.unwrap();
        assert!(
            gbm.ends_with("usr/lib/x86_64-linux-gnu/libnvidia-allocator.so.595"),
            "{}",
            gbm.display()
        );
    }

    fn context_with_mounts(mounts: &[String]) -> ContextConfig {
        let list = mounts
            .iter()
            .map(|m| format!("{m:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        ContextConfig::from_toml(
            &format!("name = \"kp\"\nprefix = \"KP\"\ncolor = \"#000000\"\nmounts = [{list}]\n"),
            Path::new("context.toml"),
        )
        .unwrap()
    }

    #[test]
    fn mounts_are_resolved_and_checked_again() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let koje_config = home.join(".config/koje");
        std::fs::create_dir_all(home.join("src/real")).unwrap();
        std::fs::create_dir_all(&koje_config).unwrap();
        std::os::unix::fs::symlink(home.join("src/real"), home.join("src/link")).unwrap();
        std::os::unix::fs::symlink(&koje_config, home.join("src/sneaky")).unwrap();
        let dirs = HostDirs {
            home: home.clone(),
            protected: vec![koje_config.clone()],
        };

        let ok = resolve_mounts(&context_with_mounts(&["~/src/link".into()]), &dirs).unwrap();
        assert_eq!(ok, [std::fs::canonicalize(home.join("src/real")).unwrap()]);

        let err =
            resolve_mounts(&context_with_mounts(&["~/src/sneaky".into()]), &dirs).unwrap_err();
        assert!(err.to_string().contains("resolves to"), "{err}");

        let err =
            resolve_mounts(&context_with_mounts(&["~/src/missing".into()]), &dirs).unwrap_err();
        assert!(matches!(err, koje_core::Error::Read { .. }), "{err}");
    }

    #[test]
    fn image_user_from_passwd_or_env() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\nbikeshedder:x:1000:1000::/home/bikeshedder:/bin/bash\n";
        let u = image_user_from(1000, passwd, |_| None);
        assert_eq!(
            (u.name.as_str(), u.home.as_path()),
            ("bikeshedder", Path::new("/home/bikeshedder"))
        );
        let u = image_user_from(1001, passwd, |k| match k {
            "USER" => Some("ldapuser".into()),
            "HOME" => Some("/home/ldapuser".into()),
            _ => None,
        });
        assert_eq!((u.name.as_str(), u.uid), ("ldapuser", 1001));
    }

    #[test]
    fn a_bare_host_has_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        // A plain file is not a socket.
        std::fs::create_dir_all(tmp.path().join("run/user/1000")).unwrap();
        std::fs::write(tmp.path().join("run/user/1000/wayland-0"), "").unwrap();
        let h = detect_with(1000, tmp.path(), |k| {
            (k == "WAYLAND_DISPLAY").then(|| "wayland-0".into())
        });
        assert_eq!(h.wayland_socket, None);
        assert_eq!(h.pipewire_socket, None);
        assert!(!h.dri);
        assert_eq!(h.nvidia, None);
    }
}
