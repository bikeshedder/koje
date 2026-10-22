//! Rootless Podman adapter: implements [`koje_core::ports::ContainerRuntime`]
//! by calling the `podman` CLI (ADR 0002; the REST API may replace it later).
//!
//! Podman is reached through a [`Runner`], so the adapter is tested with a fake
//! that records arguments and returns prepared output. cgroup and `/proc` reads
//! go through configurable roots for the same reason.

use std::path::{Path, PathBuf};
use std::process::Command;

use koje_core::container::{LABEL_CONTEXT, container_name};
use koje_core::memory::MemoryUsage;
use koje_core::name::ContextName;
use koje_core::ports::{ContainerRuntime, ContainerStatus};
use koje_core::size::ByteSize;
use koje_core::spec::{ContainerSpec, MountKind};

pub mod host;
pub mod image;
pub mod live;

/// Errors of the Podman adapter.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not run podman: {0}")]
    Spawn(#[source] std::io::Error),

    #[error("`podman {args}` failed: {stderr}")]
    Failed { args: String, stderr: String },

    #[error("unexpected {what} from podman: {value:?}")]
    Unexpected { what: &'static str, value: String },

    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Output of one `podman` invocation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `podman` with arguments.
pub trait Runner {
    fn run(&self, args: &[&str]) -> std::io::Result<RunOutput>;
    /// Run with the terminal attached (long builds show their progress);
    /// returns whether podman succeeded.
    fn run_attached(&self, args: &[&str]) -> std::io::Result<bool>;
}

/// The real `podman` binary.
#[derive(Debug, Clone)]
pub struct Cli {
    program: PathBuf,
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            program: PathBuf::from("podman"),
        }
    }
}

impl Runner for Cli {
    fn run_attached(&self, args: &[&str]) -> std::io::Result<bool> {
        tracing::debug!(?args, "podman (attached)");
        Ok(Command::new(&self.program).args(args).status()?.success())
    }

    fn run(&self, args: &[&str]) -> std::io::Result<RunOutput> {
        tracing::debug!(?args, "podman");
        let out = Command::new(&self.program).args(args).output()?;
        Ok(RunOutput {
            success: out.status.success(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }
}

/// [`ContainerRuntime`] for rootless Podman.
#[derive(Debug, Clone)]
pub struct Podman<R = Cli> {
    pub(crate) runner: R,
    cgroup_root: PathBuf,
    proc_root: PathBuf,
}

impl Podman<Cli> {
    /// The real `podman` with the host's `/sys/fs/cgroup` and `/proc`.
    pub fn new() -> Self {
        Self::with_runner(Cli::default(), "/sys/fs/cgroup", "/proc")
    }
}

impl Default for Podman<Cli> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: Runner> Podman<R> {
    pub fn with_runner(
        runner: R,
        cgroup_root: impl Into<PathBuf>,
        proc_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            runner,
            cgroup_root: cgroup_root.into(),
            proc_root: proc_root.into(),
        }
    }

    /// Run podman; trimmed stdout on success.
    pub(crate) fn podman(&self, args: &[&str]) -> Result<String> {
        let out = self.runner.run(args).map_err(Error::Spawn)?;
        if out.success {
            Ok(out.stdout.trim().to_owned())
        } else {
            Err(Error::Failed {
                args: args.join(" "),
                stderr: out.stderr.trim().to_owned(),
            })
        }
    }

    /// `podman container inspect --format <format> <target>`; `None` if there
    /// is no such container.
    fn inspect(&self, target: &str, format: &str) -> Result<Option<String>> {
        match self.podman(&["container", "inspect", "--format", format, target]) {
            Ok(value) => Ok(Some(value)),
            Err(Error::Failed { stderr, .. }) if stderr.contains("no such") => Ok(None),
            Err(e) => Err(e),
        }
    }
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|source| Error::Read {
        path: path.to_owned(),
        source,
    })
}

impl<R: Runner> Podman<R> {
    /// Run a command in the context container with the terminal attached
    /// (`podman exec -i [-t]`); returns whether it succeeded.
    pub fn exec_attached(&self, ctx: &ContextName, command: &[&str], tty: bool) -> Result<bool> {
        let name = container_name(ctx);
        let mut args = vec!["exec", "-i"];
        if tty {
            args.push("-t");
        }
        args.push(&name);
        args.extend_from_slice(command);
        self.runner.run_attached(&args).map_err(Error::Spawn)
    }

    /// Run a command in the context container and return its output (`podman exec`).
    pub fn exec_output(&self, ctx: &ContextName, command: &[&str]) -> Result<String> {
        let name = container_name(ctx);
        let mut args = vec!["exec", &name];
        args.extend_from_slice(command);
        self.podman(&args)
    }

    /// Start a command in the context container in the background (`podman exec -d`).
    pub fn exec_detached(&self, ctx: &ContextName, command: &[&str]) -> Result<()> {
        let name = container_name(ctx);
        let mut args = vec!["exec", "-d", &name];
        args.extend_from_slice(command);
        self.podman(&args).map(drop)
    }

    /// Whether a named volume exists.
    pub fn volume_exists(&self, volume: &str) -> Result<bool> {
        let out = self
            .runner
            .run(&["volume", "exists", volume])
            .map_err(Error::Spawn)?;
        Ok(out.success)
    }

    /// Remove a named volume (and everything in it).
    pub fn remove_volume(&self, volume: &str) -> Result<()> {
        self.podman(&["volume", "rm", volume]).map(drop)
    }

    /// Names of the dev containers running in the context (its nested Podman).
    pub fn dev_containers(&self, ctx: &ContextName) -> Result<Vec<String>> {
        let out = self.podman(&[
            "exec",
            &container_name(ctx),
            "podman",
            "ps",
            "--format",
            "{{.Names}}",
        ])?;
        Ok(out
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect())
    }
}

/// `koje/…` images are built locally (M3.3); Podman needs `localhost/` for them
/// to skip short-name resolution against registries.
pub fn image_ref(image: &str) -> String {
    if image.starts_with("koje/") {
        format!("localhost/{image}")
    } else {
        image.to_owned()
    }
}

/// `podman create` arguments for a spec (pure; the M2.3 `podman run` flags).
pub fn create_args(spec: &ContainerSpec) -> Vec<String> {
    let mut a: Vec<String> = ["create", "--name", &spec.name, "--hostname", &spec.hostname]
        .map(String::from)
        .into();
    // --init: a real PID 1 that forwards SIGTERM and reaps (ADR 0015);
    // keep-id: the user keeps its uid, files keep their owner (ADR 0014).
    a.extend(["--init".into(), "--userns=keep-id".into()]);
    for (k, v) in &spec.labels {
        a.extend(["--label".into(), format!("{k}={v}")]);
    }
    for opt in &spec.security_opts {
        a.extend(["--security-opt".into(), opt.clone()]);
    }
    for (k, v) in &spec.env {
        a.extend(["--env".into(), format!("{k}={v}")]);
    }
    for m in &spec.mounts {
        let target = m.target.display();
        let (flag, value) = match &m.kind {
            MountKind::Volume { name } => ("--volume", format!("{name}:{target}:U")),
            MountKind::Tmpfs { mode } => (
                "--mount",
                format!("type=tmpfs,destination={target},tmpfs-mode={mode:04o},U=true"),
            ),
            MountKind::Shared { source } => {
                ("--volume", format!("{}:{target}:rslave", source.display()))
            }
            MountKind::Bind { source, read_only } => (
                "--volume",
                format!(
                    "{}:{target}{}",
                    source.display(),
                    if *read_only { ":ro" } else { "" }
                ),
            ),
        };
        a.extend([flag.to_owned(), value]);
    }
    for d in &spec.devices {
        a.extend(["--device".into(), d.clone()]);
    }
    if !spec.network {
        a.extend(["--network".into(), "none".into()]);
    }
    a.extend(["--shm-size".into(), spec.shm_size.0.to_string()]);
    a.extend(["--workdir".into(), spec.workdir.display().to_string()]);
    a.push(image_ref(&spec.image));
    a.extend(spec.command.iter().cloned());
    a
}

/// Map Podman's `.State.Status` to koje's view.
fn parse_status(value: &str) -> Result<ContainerStatus> {
    match value {
        "running" | "stopping" => Ok(ContainerStatus::Running),
        "paused" => Ok(ContainerStatus::Paused),
        "created" | "configured" | "exited" | "stopped" | "initialized" => {
            Ok(ContainerStatus::Stopped)
        }
        other => Err(Error::Unexpected {
            what: "container status",
            value: other.to_owned(),
        }),
    }
}

/// The container id in a `/proc/<pid>/cgroup` file, from systemd's
/// `libpod-<64 hex>.scope` (or cgroupfs' `libpod-<64 hex>`). Conmon's
/// `libpod-conmon-<id>.scope` is not a container process and is skipped.
pub fn container_id_from_cgroup(text: &str) -> Option<&str> {
    text.match_indices("libpod-").find_map(|(i, _)| {
        let rest = &text[i + "libpod-".len()..];
        let id = rest.get(..64)?;
        id.bytes().all(|b| b.is_ascii_hexdigit()).then_some(id)
    })
}

impl<R: Runner> Podman<R> {
    /// The cgroup of a running context: Podman's `…/libpod-<id>.scope`. Its
    /// `CgroupPath` ends in `/container`, which holds only the container's
    /// init; programs started with `podman exec` are elsewhere in the scope
    /// (host test, M6) – the scope counts and limits them all.
    fn scope_dir(&self, ctx: &ContextName) -> Result<PathBuf> {
        let name = container_name(ctx);
        let cgroup = self
            .inspect(&name, "{{.State.CgroupPath}}")?
            .filter(|p| !p.is_empty())
            .ok_or(Error::Unexpected {
                what: "cgroup path (container not running?)",
                value: name,
            })?;
        let dir = self.cgroup_root.join(cgroup.trim_start_matches('/'));
        Ok(match dir.parent() {
            Some(scope) if dir.file_name().is_some_and(|n| n == "container") => scope.to_path_buf(),
            _ => dir,
        })
    }

    /// The kernel memory limit of a running context (`memory.max` of its
    /// scope); `None` for no limit.
    pub fn kernel_limit(&self, ctx: &ContextName) -> Result<Option<ByteSize>> {
        let path = self.scope_dir(ctx)?.join("memory.max");
        let text = read(&path)?;
        let t = text.trim();
        if t == "max" {
            return Ok(None);
        }
        t.parse::<u64>()
            .map(|b| Some(ByteSize(b)))
            .map_err(|_| Error::Unexpected {
                what: "memory.max",
                value: t.to_owned(),
            })
    }

    /// Set the kernel memory limit of a running context (ADR 0035): koje writes
    /// its scope's `memory.max` itself (delegated to the user) – Podman's own
    /// limit gets in the way (`podman update` refuses "unlimited" and stores
    /// swap values crun then refuses at start). Lost at a restart; `koje up`
    /// writes it again.
    pub fn set_kernel_limit(&self, ctx: &ContextName, max: Option<ByteSize>) -> Result<()> {
        let path = self.scope_dir(ctx)?.join("memory.max");
        let value = max.map_or_else(|| "max".to_owned(), |b| b.0.to_string());
        std::fs::write(&path, value).map_err(|source| Error::Read { path, source })
    }
}

impl<R: Runner> ContainerRuntime for Podman<R> {
    type Error = Error;

    fn status(&self, ctx: &ContextName) -> Result<ContainerStatus> {
        match self.inspect(&container_name(ctx), "{{.State.Status}}")? {
            None => Ok(ContainerStatus::Missing),
            Some(value) => parse_status(&value),
        }
    }

    fn create(&self, spec: &ContainerSpec) -> Result<()> {
        let args = create_args(spec);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        self.podman(&args).map(drop)
    }

    fn remove(&self, ctx: &ContextName) -> Result<()> {
        self.podman(&["rm", &container_name(ctx)]).map(drop)
    }

    fn start(&self, ctx: &ContextName) -> Result<()> {
        self.podman(&["start", &container_name(ctx)]).map(drop)
    }

    fn stop(&self, ctx: &ContextName) -> Result<()> {
        self.podman(&["stop", "--time", "10", &container_name(ctx)])
            .map(drop)
    }

    fn pause(&self, ctx: &ContextName) -> Result<()> {
        self.podman(&["pause", &container_name(ctx)]).map(drop)
    }

    fn unpause(&self, ctx: &ContextName) -> Result<()> {
        self.podman(&["unpause", &container_name(ctx)]).map(drop)
    }

    fn memory_usage(&self, ctx: &ContextName) -> Result<MemoryUsage> {
        let dir = self.scope_dir(ctx)?;
        let text = read(&dir.join("memory.current"))?;
        let current = text
            .trim()
            .parse::<u64>()
            .map(ByteSize)
            .map_err(|_| Error::Unexpected {
                what: "memory.current",
                value: text.trim().to_owned(),
            })?;
        // Without memory.stat (unusual): everything counts.
        let (anon, inactive_file) = read(&dir.join("memory.stat"))
            .map(|t| koje_core::memory::parse_stat(&t))
            .unwrap_or_default();
        Ok(MemoryUsage {
            current,
            anon,
            inactive_file,
        })
    }

    fn launch(&self, ctx: &ContextName, command: &str, workdir: Option<&str>) -> Result<()> {
        let name = container_name(ctx);
        let mut args = vec!["exec", "-d"];
        if let Some(w) = workdir {
            args.extend(["--workdir", w]);
        }
        args.extend([name.as_str(), "sh", "-c", command]);
        self.podman(&args).map(drop)
    }

    fn query(&self, ctx: &ContextName, argv: &[&str]) -> Result<String> {
        self.exec_output(ctx, argv)
    }

    fn terminate(&self, pid: u32) -> Result<()> {
        // The context's processes run as the user (keep-id): a plain kill works.
        let out = std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .output()
            .map_err(Error::Spawn)?;
        if out.status.success() {
            Ok(())
        } else {
            Err(Error::Failed {
                args: format!("kill -TERM {pid}"),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            })
        }
    }

    fn context_of_pid(&self, pid: u32) -> Result<Option<ContextName>> {
        let path = self.proc_root.join(pid.to_string()).join("cgroup");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            // The process is gone: it belongs to no context any more.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(Error::Read { path, source }),
        };
        let Some(id) = container_id_from_cgroup(&text) else {
            return Ok(None);
        };
        let format = format!("{{{{index .Config.Labels \"{LABEL_CONTEXT}\"}}}}");
        let label = self.inspect(id, &format)?.unwrap_or_default();
        if label.is_empty() || label == "<no value>" {
            return Ok(None); // a container, but not one of koje's
        }
        ContextName::new(&label)
            .map(Some)
            .map_err(|_| Error::Unexpected {
                what: "context label",
                value: label,
            })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use super::*;

    /// Records every call and answers from a queue.
    #[derive(Default)]
    pub(crate) struct Fake {
        pub(crate) calls: RefCell<Vec<String>>,
        answers: RefCell<VecDeque<RunOutput>>,
    }

    impl Fake {
        pub(crate) fn answer(self, success: bool, stdout: &str, stderr: &str) -> Self {
            self.answers.borrow_mut().push_back(RunOutput {
                success,
                stdout: stdout.into(),
                stderr: stderr.into(),
            });
            self
        }
    }

    impl Runner for &Fake {
        fn run_attached(&self, args: &[&str]) -> std::io::Result<bool> {
            Ok(self.run(args)?.success)
        }

        fn run(&self, args: &[&str]) -> std::io::Result<RunOutput> {
            self.calls.borrow_mut().push(args.join(" "));
            Ok(self.answers.borrow_mut().pop_front().unwrap_or(RunOutput {
                success: true,
                ..RunOutput::default()
            }))
        }
    }

    fn kp() -> ContextName {
        ContextName::new("kp").unwrap()
    }

    const ID: &str = "275e5400c5b8d0009562fa78c0a675959f3e3e8ab12b81d4c9fc4c29240fc160";

    #[test]
    fn status_maps_podman_states() {
        for (value, want) in [
            ("running", ContainerStatus::Running),
            ("paused", ContainerStatus::Paused),
            ("exited", ContainerStatus::Stopped),
            ("created", ContainerStatus::Stopped),
        ] {
            let fake = Fake::default().answer(true, &format!("{value}\n"), "");
            let podman = Podman::with_runner(&fake, "/nonexistent", "/nonexistent");
            assert_eq!(podman.status(&kp()).unwrap(), want, "{value}");
            assert_eq!(
                fake.calls.borrow()[0],
                "container inspect --format {{.State.Status}} koje-kp"
            );
        }
        let fake = Fake::default().answer(true, "weird", "");
        assert!(matches!(
            Podman::with_runner(&fake, "/x", "/x").status(&kp()),
            Err(Error::Unexpected { .. })
        ));
    }

    #[test]
    fn missing_container_is_a_status_not_an_error() {
        let fake = Fake::default().answer(false, "", "Error: no such container koje-kp\n");
        let podman = Podman::with_runner(&fake, "/x", "/x");
        assert_eq!(podman.status(&kp()).unwrap(), ContainerStatus::Missing);

        let fake = Fake::default().answer(false, "", "Error: cannot connect to Podman\n");
        assert!(matches!(
            Podman::with_runner(&fake, "/x", "/x").status(&kp()),
            Err(Error::Failed { .. })
        ));
    }

    #[test]
    fn lifecycle_commands() {
        let fake = Fake::default();
        let podman = Podman::with_runner(&fake, "/x", "/x");
        podman.start(&kp()).unwrap();
        podman.stop(&kp()).unwrap();
        podman.pause(&kp()).unwrap();
        podman.unpause(&kp()).unwrap();
        assert_eq!(
            *fake.calls.borrow(),
            [
                "start koje-kp",
                "stop --time 10 koje-kp",
                "pause koje-kp",
                "unpause koje-kp"
            ]
        );
    }

    #[test]
    fn failures_carry_podman_stderr() {
        let fake = Fake::default().answer(false, "", "Error: container is not running\n");
        let err = Podman::with_runner(&fake, "/x", "/x")
            .pause(&kp())
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "`podman pause koje-kp` failed: Error: container is not running"
        );
    }

    #[test]
    fn kernel_limit_in_the_scope() {
        let tmp = tempfile::tempdir().unwrap();
        let scope = "user.slice/libpod-abc.scope";
        std::fs::create_dir_all(tmp.path().join(scope).join("container")).unwrap();
        std::fs::write(tmp.path().join(scope).join("memory.max"), "max\n").unwrap();
        let answer = format!("/{scope}/container\n");
        let fake = Fake::default()
            .answer(true, &answer, "")
            .answer(true, &answer, "")
            .answer(true, &answer, "")
            .answer(true, &answer, "");
        let podman = Podman::with_runner(&fake, tmp.path(), "/x");
        assert_eq!(podman.kernel_limit(&kp()).unwrap(), None);
        podman
            .set_kernel_limit(&kp(), Some(ByteSize(3 << 30)))
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join(scope).join("memory.max")).unwrap(),
            (3u64 << 30).to_string(),
            "the scope, not the init's cgroup"
        );
        assert_eq!(podman.kernel_limit(&kp()).unwrap(), Some(ByteSize(3 << 30)));
        podman.set_kernel_limit(&kp(), None).unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join(scope).join("memory.max")).unwrap(),
            "max"
        );
    }

    /// Host test of M6: `CgroupPath` ends in `/container`, which holds only
    /// the init; the scope above it has the programs.
    #[test]
    fn memory_usage_reads_the_scope_not_the_init_cgroup() {
        let tmp = tempfile::tempdir().unwrap();
        let scope = "user.slice/libpod-abc.scope";
        std::fs::create_dir_all(tmp.path().join(scope).join("container")).unwrap();
        std::fs::write(
            tmp.path().join(scope).join("memory.current"),
            "2000000000\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join(scope).join("container/memory.current"),
            "3000000\n",
        )
        .unwrap();
        let fake = Fake::default().answer(true, &format!("/{scope}/container\n"), "");
        let podman = Podman::with_runner(&fake, tmp.path(), "/x");
        assert_eq!(
            podman.memory_usage(&kp()).unwrap().current,
            ByteSize(2_000_000_000)
        );
    }

    #[test]
    fn memory_usage_reads_the_cgroup() {
        let tmp = tempfile::tempdir().unwrap();
        let scope = "user.slice/user-1000.slice/libpod-abc.scope";
        std::fs::create_dir_all(tmp.path().join(scope)).unwrap();
        std::fs::write(
            tmp.path().join(scope).join("memory.current"),
            "1019215872\n",
        )
        .unwrap();
        let fake = Fake::default().answer(true, &format!("/{scope}\n"), "");
        let podman = Podman::with_runner(&fake, tmp.path(), "/x");
        std::fs::write(
            tmp.path().join(scope).join("memory.stat"),
            "anon 500000000\ninactive_file 300000000\n",
        )
        .unwrap();
        let u = podman.memory_usage(&kp()).unwrap();
        assert_eq!(u.current, ByteSize(1_019_215_872));
        assert_eq!(u.working_set(), ByteSize(719_215_872));
        assert_eq!(u.anon, ByteSize(500_000_000));
    }

    fn spec() -> ContainerSpec {
        use koje_core::access::Profile;
        use koje_core::config::Limits;
        use koje_core::spec::{ContextInputs, HostEnv, build};
        let host = HostEnv {
            uid: 1000,
            home: "/home/mj".into(),
            wayland_socket: Some("/run/user/1000/koje/kp/wayland/wayland-0".into()),
            pipewire_socket: Some("/run/user/1000/pipewire-0".into()),
            pulse_socket: Some("/run/user/1000/pulse/native".into()),
            sound_dir: Some("/run/user/1000/koje/kp/sound".into()),
            dri: true,
            ..HostEnv::default()
        };
        let limits = Limits {
            memory_soft: None,
            memory_hard: Some(ByteSize(8 << 30)),
        };
        let name = kp();
        build(
            &ContextInputs {
                name: &name,
                image: "koje/base:latest",
                access: Profile::Full.access(),
                limits: &limits,
                mounts: &["/home/mj/src/kp".into()],
            },
            &host,
        )
        .spec
    }

    #[test]
    fn create_args_match_the_m2_3_flags() {
        let args = create_args(&spec()).join(" ");
        for part in [
            "create --name koje-kp --hostname kp --init --userns=keep-id",
            "--label io.koje.context=kp",
            "--security-opt label=disable --security-opt unmask=/proc/sys",
            "--env PULSE_SERVER=unix:/run/user/1000/koje-sound/pulse-native",
            "--env WAYLAND_DISPLAY=/run/user/1000/koje-wayland/wayland-0",
            "--volume koje-kp-home:/home/mj:U",
            "--mount type=tmpfs,destination=/run/user/1000,tmpfs-mode=0700,U=true",
            "--volume /run/user/1000/koje/kp/wayland:/run/user/1000/koje-wayland",
            "--volume /run/user/1000/koje/kp/sound:/run/user/1000/koje-sound",
            "--volume /home/mj/src/kp:/home/mj/src/kp",
            "--device /dev/dri --device /dev/fuse --device /dev/net/tun",
            "--shm-size 1073741824",
            "--workdir /home/mj",
            "localhost/koje/base:latest dbus-daemon --session --nofork --nopidfile --address=unix:path=/run/user/1000/bus",
        ] {
            assert!(args.contains(part), "missing {part:?} in\n{args}");
        }
        assert!(!args.contains("--network"), "{args}");
    }

    #[test]
    fn create_args_without_network_and_read_only_binds() {
        let mut s = spec();
        s.network = false;
        s.mounts.push(koje_core::spec::SpecMount {
            target: "/usr/lib64/gbm/nvidia-drm_gbm.so".into(),
            kind: MountKind::Bind {
                source: "/usr/lib/x86_64-linux-gnu/libnvidia-allocator.so.1".into(),
                read_only: true,
            },
        });
        let args = create_args(&s).join(" ");
        assert!(args.contains("--network none"), "{args}");
        assert!(
            args.contains("libnvidia-allocator.so.1:/usr/lib64/gbm/nvidia-drm_gbm.so:ro"),
            "{args}"
        );
    }

    #[test]
    fn image_refs() {
        assert_eq!(image_ref("koje/base:latest"), "localhost/koje/base:latest");
        assert_eq!(
            image_ref("registry.fedoraproject.org/fedora:44"),
            "registry.fedoraproject.org/fedora:44"
        );
    }

    #[test]
    fn create_and_remove_call_podman() {
        let fake = Fake::default();
        let podman = Podman::with_runner(&fake, "/x", "/x");
        podman.create(&spec()).unwrap();
        podman.remove(&kp()).unwrap();
        let calls = fake.calls.borrow();
        assert!(
            calls[0].starts_with("create --name koje-kp"),
            "{}",
            calls[0]
        );
        assert_eq!(calls[1], "rm koje-kp");
    }

    #[test]
    fn launch_execs_detached_in_the_workdir() {
        let fake = Fake::default();
        let podman = Podman::with_runner(&fake, "/x", "/x");
        podman.launch(&kp(), "foot", Some("/tmp")).unwrap();
        podman.launch(&kp(), "evince 'a b.pdf'", None).unwrap();
        let calls = fake.calls.borrow();
        assert_eq!(calls[0], "exec -d --workdir /tmp koje-kp sh -c foot");
        assert_eq!(calls[1], "exec -d koje-kp sh -c evince 'a b.pdf'");
    }

    #[test]
    fn exec_and_dev_containers() {
        let fake = Fake::default()
            .answer(true, "", "")
            .answer(true, "", "")
            .answer(true, "web\ndb\n", "");
        let podman = Podman::with_runner(&fake, "/x", "/x");
        assert!(podman.exec_attached(&kp(), &["bash", "-l"], true).unwrap());
        podman
            .exec_detached(&kp(), &["xdg-open", "https://example.org"])
            .unwrap();
        assert_eq!(podman.dev_containers(&kp()).unwrap(), ["web", "db"]);
        assert_eq!(
            *fake.calls.borrow(),
            [
                "exec -i -t koje-kp bash -l",
                "exec -d koje-kp xdg-open https://example.org",
                "exec koje-kp podman ps --format {{.Names}}"
            ]
        );
    }

    #[test]
    fn container_id_from_systemd_and_cgroupfs_paths() {
        let systemd = format!(
            "0::/user.slice/user-1000.slice/user@1000.service/user.slice/libpod-{ID}.scope/container\n"
        );
        assert_eq!(container_id_from_cgroup(&systemd), Some(ID));
        let cgroupfs = format!("0::/libpod_parent/libpod-{ID}\n");
        assert_eq!(container_id_from_cgroup(&cgroupfs), Some(ID));
        let conmon = format!("0::/user.slice/libpod-conmon-{ID}.scope\n");
        assert_eq!(container_id_from_cgroup(&conmon), None);
        assert_eq!(
            container_id_from_cgroup("0::/user.slice/app.slice/foot.scope\n"),
            None
        );
    }

    fn proc_with_cgroup(pid: u32, text: &str) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(pid.to_string())).unwrap();
        std::fs::write(tmp.path().join(pid.to_string()).join("cgroup"), text).unwrap();
        tmp
    }

    #[test]
    fn context_of_pid_follows_cgroup_and_label() {
        let proc = proc_with_cgroup(42, &format!("0::/user.slice/libpod-{ID}.scope/container\n"));
        let fake = Fake::default().answer(true, "kundenportal\n", "");
        let podman = Podman::with_runner(&fake, "/x", proc.path());
        assert_eq!(
            podman.context_of_pid(42).unwrap(),
            Some(ContextName::new("kundenportal").unwrap())
        );
        assert_eq!(
            fake.calls.borrow()[0],
            format!(
                "container inspect --format {{{{index .Config.Labels \"io.koje.context\"}}}} {ID}"
            )
        );
    }

    #[test]
    fn context_of_pid_is_none_for_host_and_foreign_processes() {
        // Host process: no libpod cgroup, podman is not even asked.
        let proc = proc_with_cgroup(7, "0::/user.slice/app.slice/foot.scope\n");
        let fake = Fake::default();
        let podman = Podman::with_runner(&fake, "/x", proc.path());
        assert_eq!(podman.context_of_pid(7).unwrap(), None);
        assert!(fake.calls.borrow().is_empty());
        // Gone process.
        assert_eq!(podman.context_of_pid(8).unwrap(), None);
        // A container without koje's label.
        let proc = proc_with_cgroup(9, &format!("0::/libpod-{ID}.scope\n"));
        for label in ["", "<no value>"] {
            let fake = Fake::default().answer(true, label, "");
            let podman = Podman::with_runner(&fake, "/x", proc.path());
            assert_eq!(podman.context_of_pid(9).unwrap(), None, "{label:?}");
        }
    }
}
