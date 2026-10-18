//! Tests against a real rootless Podman. Ignored by default; run on the host:
//!
//!   cargo test -p koje-podman -- --ignored
//!
//! They only create containers labelled `io.koje.test=true` and remove them
//! again, also when a test fails. They need `docker.io/library/alpine`
//! (pulled on first use).

#![allow(clippy::expect_used)]

use std::process::Command;

use koje_core::container::{LABEL_CONTEXT, LABEL_TEST, container_name};
use koje_core::name::ContextName;
use koje_core::ports::{ContainerRuntime, ContainerStatus};
use koje_podman::Podman;

/// A throwaway container `koje-kojetest-<pid>`, removed on drop.
struct TestContainer {
    ctx: ContextName,
}

impl TestContainer {
    fn start() -> Self {
        let ctx = ContextName::new(format!("kojetest-{}", std::process::id())).expect("valid name");
        let out = Command::new("podman")
            // --init: `sleep` as PID 1 would ignore SIGTERM (10 s per stop).
            .args(["run", "-d", "--init", "--name", &container_name(&ctx)])
            .args(["--label", &format!("{LABEL_TEST}=true")])
            .args(["--label", &format!("{LABEL_CONTEXT}={ctx}")])
            .args(["docker.io/library/alpine", "sleep", "300"])
            .output()
            .expect("run podman");
        assert!(
            out.status.success(),
            "podman run failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        Self { ctx }
    }

    fn host_pid(&self) -> u32 {
        let out = Command::new("podman")
            .args(["container", "inspect", "--format", "{{.State.Pid}}"])
            .arg(container_name(&self.ctx))
            .output()
            .expect("inspect");
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse()
            .expect("pid")
    }
}

impl Drop for TestContainer {
    fn drop(&mut self) {
        // Only the container this test run created: the name is unique to it.
        let _ = Command::new("podman")
            .args(["rm", "-f", "--time", "0", &container_name(&self.ctx)])
            .output();
    }
}

#[test]
#[ignore = "needs rootless Podman on the host"]
fn lifecycle_memory_and_pid_mapping() {
    let podman = Podman::new();
    let missing = ContextName::new("kojetest-does-not-exist").expect("valid name");
    assert_eq!(
        podman.status(&missing).expect("status"),
        ContainerStatus::Missing
    );

    let c = TestContainer::start();
    assert_eq!(
        podman.status(&c.ctx).expect("status"),
        ContainerStatus::Running
    );

    let memory = podman.memory_usage(&c.ctx).expect("memory");
    assert!(memory.0 > 0, "memory.current should be positive");

    let pid = c.host_pid();
    assert_eq!(
        podman.context_of_pid(pid).expect("context of pid"),
        Some(c.ctx.clone())
    );
    // This test process itself runs on the host, not in a context.
    assert_eq!(
        podman.context_of_pid(std::process::id()).expect("own pid"),
        None
    );

    podman.pause(&c.ctx).expect("pause");
    assert_eq!(
        podman.status(&c.ctx).expect("status"),
        ContainerStatus::Paused
    );
    podman.unpause(&c.ctx).expect("unpause");
    assert_eq!(
        podman.status(&c.ctx).expect("status"),
        ContainerStatus::Running
    );

    podman.stop(&c.ctx).expect("stop");
    assert_eq!(
        podman.status(&c.ctx).expect("status"),
        ContainerStatus::Stopped
    );
    podman.start(&c.ctx).expect("start");
    assert_eq!(
        podman.status(&c.ctx).expect("status"),
        ContainerStatus::Running
    );
}

/// A context container created from a spec, removed (with its home volume) on drop.
struct SpecContainer {
    ctx: ContextName,
}

impl Drop for SpecContainer {
    fn drop(&mut self) {
        let _ = Command::new("podman")
            .args(["rm", "-f", "--time", "0", &container_name(&self.ctx)])
            .output();
        let _ = Command::new("podman")
            .args([
                "volume",
                "rm",
                "-f",
                &koje_core::container::home_volume(&self.ctx),
            ])
            .output();
    }
}

#[test]
#[ignore = "needs rootless Podman on the host"]
fn create_from_spec_keeps_uid_and_isolates_network() {
    use koje_core::access::Profile;
    use koje_core::config::Limits;
    use koje_core::spec::{ContextInputs, build};

    let ctx = ContextName::new(format!("kojespec-{}", std::process::id())).expect("valid name");
    let mut host = koje_podman::host::detect().expect("detect host");
    // No real context resources in this test: isolated profile, no host bus.
    host.host_bus_dir = None;
    let limits = Limits::default();
    let mut spec = build(
        &ContextInputs {
            name: &ctx,
            image: "docker.io/library/alpine",
            access: Profile::Isolated.access(),
            limits: &limits,
            mounts: &[],
        },
        &host,
    )
    .spec;
    // The koje base image (M3.3) does not exist yet: alpine with sleep instead of dbus-daemon.
    spec.command = vec!["sleep".into(), "300".into()];
    spec.labels.insert(LABEL_TEST.into(), "true".into());

    let podman = Podman::new();
    let _cleanup = SpecContainer { ctx: ctx.clone() };
    podman.create(&spec).expect("create");
    assert_eq!(
        podman.status(&ctx).expect("status"),
        ContainerStatus::Stopped
    );
    podman.start(&ctx).expect("start");
    assert_eq!(
        podman.status(&ctx).expect("status"),
        ContainerStatus::Running
    );

    let exec = |args: &[&str]| {
        let out = Command::new("podman")
            .args(["exec", &container_name(&ctx)])
            .args(args)
            .output()
            .expect("exec");
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    assert_eq!(
        exec(&["id", "-u"]),
        host.uid.to_string(),
        "keep-id: same uid"
    );
    assert_eq!(
        exec(&["sh", "-c", "echo $HOME"]),
        host.home.display().to_string()
    );
    // Isolated: only the loopback interface.
    let links = exec(&["ls", "/sys/class/net"]);
    assert_eq!(links, "lo", "isolated profile must have no network");

    podman.stop(&ctx).expect("stop");
    podman.remove(&ctx).expect("remove");
    assert_eq!(
        podman.status(&ctx).expect("status"),
        ContainerStatus::Missing
    );
}

#[test]
#[ignore = "needs rootless Podman and koje/base (koje image build) on the host"]
fn base_image_runs_the_session_bus() {
    use koje_core::access::Profile;
    use koje_core::config::Limits;
    use koje_core::spec::{ContextInputs, build};

    let podman = Podman::new();
    assert!(
        podman
            .image_exists(koje_podman::image::BASE_IMAGE)
            .expect("image exists"),
        "build it first: koje image build"
    );
    let ctx = ContextName::new(format!("kojebase-{}", std::process::id())).expect("valid name");
    let host = koje_podman::host::detect().expect("detect host");
    let limits = Limits::default();
    let mut spec = build(
        &ContextInputs {
            name: &ctx,
            image: koje_podman::image::BASE_IMAGE,
            access: Profile::Isolated.access(),
            limits: &limits,
            mounts: &[],
        },
        &host,
    )
    .spec;
    spec.labels.insert(LABEL_TEST.into(), "true".into());
    let _cleanup = SpecContainer { ctx: ctx.clone() };
    podman.create(&spec).expect("create");
    podman.start(&ctx).expect("start");

    let exec = |args: &[&str]| {
        Command::new("podman")
            .args(["exec", &container_name(&ctx)])
            .args(args)
            .output()
            .expect("exec")
    };
    // The main process is the context's own session bus (ADR 0015).
    let mut names = exec(&[
        "dbus-send",
        "--session",
        "--print-reply",
        "--dest=org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus.ListNames",
    ]);
    for _ in 0..20 {
        if names.status.success() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        names = exec(&[
            "dbus-send",
            "--session",
            "--print-reply",
            "--dest=org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus.ListNames",
        ]);
    }
    assert!(
        names.status.success(),
        "session bus: {}",
        String::from_utf8_lossy(&names.stderr)
    );
    // Nested Podman's id mapping tools have their file capabilities (ADR 0014).
    let caps = exec(&["getcap", "/usr/bin/newuidmap"]);
    assert!(
        String::from_utf8_lossy(&caps.stdout).contains("cap_setuid"),
        "newuidmap caps"
    );
    // The image's portal is not D-Bus activatable (ADR 0019/0023).
    let activatable = exec(&[
        "dbus-send",
        "--session",
        "--print-reply",
        "--dest=org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus.ListActivatableNames",
    ]);
    assert!(
        !String::from_utf8_lossy(&activatable.stdout).contains("org.freedesktop.portal.Desktop"),
        "the image's portal must not be activatable"
    );
}
