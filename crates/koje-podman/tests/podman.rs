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
