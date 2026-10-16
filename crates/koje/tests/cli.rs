//! End-to-end tests of the `koje` binary with isolated XDG directories.
//! They never touch the real user configuration, Podman or Hyprland.

#![allow(clippy::expect_used)]

use std::path::Path;
use std::process::{Command, Output};

fn koje(xdg: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_koje"))
        .args(args)
        .env("XDG_CONFIG_HOME", xdg.join("config"))
        .env("XDG_DATA_HOME", xdg.join("data"))
        .env("XDG_STATE_HOME", xdg.join("state"))
        .env_remove("RUST_LOG")
        .output()
        .expect("run koje")
}

#[test]
fn list_without_contexts() {
    let tmp = tempfile::tempdir().unwrap();
    let out = koje(tmp.path(), &["list"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("no contexts yet"));
}

#[test]
fn list_shows_configured_contexts() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("config/koje/contexts/kundenportal");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("context.toml"),
        "name = \"kundenportal\"\nprefix = \"KP\"\ncolor = \"#2E7D5B\"\n",
    )
    .unwrap();

    let out = koje(tmp.path(), &["list"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("kundenportal"));
    assert!(stdout.contains("#2E7D5B"));
}

#[test]
fn unimplemented_commands_name_their_milestone() {
    let tmp = tempfile::tempdir().unwrap();
    let out = koje(tmp.path(), &["up", "kp"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("milestone M3"));
}

#[test]
fn usage_errors_exit_with_code_2() {
    let tmp = tempfile::tempdir().unwrap();
    let out = koje(tmp.path(), &["down", "kp", "-s", "x", "--no-save"]);
    assert_eq!(out.status.code(), Some(2));
}
