//! End-to-end tests of the `koje` binary with isolated XDG directories.
//! They never touch the real user configuration, Podman or Hyprland.

#![allow(clippy::expect_used)]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn command(xdg: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_koje"));
    cmd.args(args)
        .env("XDG_CONFIG_HOME", xdg.join("config"))
        .env("XDG_DATA_HOME", xdg.join("data"))
        .env("XDG_STATE_HOME", xdg.join("state"))
        .env_remove("RUST_LOG")
        .env_remove("VISUAL")
        .env_remove("EDITOR");
    cmd
}

fn koje(xdg: &Path, args: &[&str]) -> Output {
    command(xdg, args)
        .stdin(Stdio::null())
        .output()
        .expect("run koje")
}

/// Run koje with `input` on stdin and extra environment variables.
fn koje_with(xdg: &Path, args: &[&str], env: &[(&str, &str)], input: &str) -> Output {
    let mut child = command(xdg, args)
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn koje");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write stdin");
    child.wait_with_output().expect("wait for koje")
}

/// A fake editor: a shell script that runs `body` with the file as `$1`.
/// Returned as `sh <script>`; executing a just-written file directly can fail
/// with "text file busy" while other test threads fork.
fn editor(dir: &Path, body: &str) -> String {
    let path = dir.join("fake-editor.sh");
    std::fs::write(&path, format!("set -e\n{body}\n")).expect("write fake editor");
    format!("sh {}", path.display())
}

fn config_of(xdg: &Path, ctx: &str) -> PathBuf {
    xdg.join(format!("config/koje/contexts/{ctx}/context.toml"))
}

#[test]
fn list_without_contexts() {
    let tmp = tempfile::tempdir().unwrap();
    let out = koje(tmp.path(), &["list"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("no contexts yet"));
    let json = stdout(&koje(tmp.path(), &["list", "--json"]));
    assert_eq!(json.trim(), "[]");
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

fn stdout(out: &Output) -> String {
    assert!(
        out.status.success(),
        "koje failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn new_creates_context_with_defaults() {
    let tmp = tempfile::tempdir().unwrap();
    let out = stdout(&koje(tmp.path(), &["new", "kunden-portal"]));
    assert!(out.contains("KP"), "{out}");
    assert!(out.contains("#2E7D5B"), "{out}");
    let file = tmp
        .path()
        .join("config/koje/contexts/kunden-portal/context.toml");
    let text = std::fs::read_to_string(file).unwrap();
    assert!(text.contains("image  = \"koje/base:latest\""), "{text}");

    // The second context gets the next palette color.
    let out = stdout(&koje(tmp.path(), &["new", "shop", "--prefix", "SH"]));
    assert!(out.contains("SH, #1E88E5"), "{out}");
    let listed = stdout(&koje(tmp.path(), &["list"]));
    assert!(
        listed.contains("kunden-portal") && listed.contains("shop"),
        "{listed}"
    );
}

#[test]
fn new_refuses_existing_and_invalid_input() {
    let tmp = tempfile::tempdir().unwrap();
    stdout(&koje(tmp.path(), &["new", "kp", "--color", "#123456"]));
    for args in [
        &["new", "kp"][..],
        &["new", "Bad Name"],
        &["new", "other", "--color", "red"],
    ] {
        let out = koje(tmp.path(), args);
        assert!(!out.status.success(), "{args:?}");
        assert!(String::from_utf8_lossy(&out.stderr).starts_with("koje: "));
    }
}

#[test]
fn edit_saves_valid_changes() {
    let tmp = tempfile::tempdir().unwrap();
    stdout(&koje(tmp.path(), &["new", "kp"]));
    let ed = editor(
        tmp.path(),
        r#"sed -i 's/^prefix = "KP"/prefix = "XY"/' "$1""#,
    );
    let out = koje_with(tmp.path(), &["edit", "kp"], &[("VISUAL", ed.as_str())], "");
    assert!(stdout(&out).contains("saved"));
    let text = std::fs::read_to_string(config_of(tmp.path(), "kp")).unwrap();
    assert!(text.contains("prefix = \"XY\""), "{text}");
    // No draft file is left behind.
    let dir = config_of(tmp.path(), "kp").parent().unwrap().to_owned();
    assert_eq!(std::fs::read_dir(dir).unwrap().count(), 1);
}

#[test]
fn edit_keeps_original_when_invalid_and_not_reopened() {
    let tmp = tempfile::tempdir().unwrap();
    stdout(&koje(tmp.path(), &["new", "kp"]));
    let before = std::fs::read_to_string(config_of(tmp.path(), "kp")).unwrap();
    let ed = editor(tmp.path(), r#"echo 'mounts = ["relative"]' > "$1""#);
    let out = koje_with(
        tmp.path(),
        &["edit", "kp"],
        &[("EDITOR", ed.as_str())],
        "n\n",
    );
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unchanged"), "{stderr}");
    let after = std::fs::read_to_string(config_of(tmp.path(), "kp")).unwrap();
    assert_eq!(before, after);
}

#[test]
fn edit_reopens_until_valid() {
    let tmp = tempfile::tempdir().unwrap();
    stdout(&koje(tmp.path(), &["new", "kp"]));
    // First run breaks the file, the second run (marker exists) fixes the prefix.
    let marker = tmp.path().join("marker");
    let ed = editor(
        tmp.path(),
        &format!(
            r#"if [ -e {m} ]; then sed -i 's/^name .*/name = "kp"/; s/^prefix = "KP"/prefix = "OK"/' "$1"; else touch {m}; sed -i 's/^name .*/name = "other"/' "$1"; fi"#,
            m = marker.display()
        ),
    );
    let out = koje_with(
        tmp.path(),
        &["edit", "kp"],
        &[("VISUAL", ed.as_str())],
        "\n",
    );
    assert!(stdout(&out).contains("saved"));
    assert!(String::from_utf8_lossy(&out.stderr).contains("directory"));
    let text = std::fs::read_to_string(config_of(tmp.path(), "kp")).unwrap();
    assert!(text.contains("prefix = \"OK\""), "{text}");
}

#[test]
fn rm_asks_unless_yes() {
    let tmp = tempfile::tempdir().unwrap();
    stdout(&koje(tmp.path(), &["new", "kp"]));
    let state = tmp.path().join("data/koje/kp/states/default.toml");
    std::fs::create_dir_all(state.parent().unwrap()).unwrap();
    std::fs::write(&state, "active_layout = \"main\"\n").unwrap();

    let out = koje_with(tmp.path(), &["rm", "kp"], &[], "n\n");
    assert!(!out.status.success());
    assert!(config_of(tmp.path(), "kp").exists());

    // No terminal input at all also means no.
    assert!(!koje(tmp.path(), &["rm", "kp"]).status.success());
    assert!(config_of(tmp.path(), "kp").exists());

    let out = koje_with(tmp.path(), &["rm", "kp"], &[], "y\n");
    assert!(stdout(&out).contains("removed"));
    assert!(!config_of(tmp.path(), "kp").exists());
    assert!(!state.exists());

    stdout(&koje(tmp.path(), &["new", "shop"]));
    stdout(&koje(tmp.path(), &["rm", "shop", "--yes"]));
    assert!(!koje(tmp.path(), &["rm", "shop", "--yes"]).status.success());
}

#[test]
fn state_commands_follow_adr_0013() {
    let tmp = tempfile::tempdir().unwrap();
    stdout(&koje(tmp.path(), &["new", "kp"]));
    let listed = stdout(&koje(tmp.path(), &["state", "list", "kp"]));
    assert!(listed.contains("default (not saved yet)"), "{listed}");
    assert!(listed.contains("reserved"), "{listed}");

    stdout(&koje(
        tmp.path(),
        &["state", "cp", "kp", "empty", "default"],
    ));
    stdout(&koje(
        tmp.path(),
        &["state", "cp", "kp", "default", "workshop"],
    ));
    let listed = stdout(&koje(tmp.path(), &["state", "list", "kp"]));
    assert!(listed.contains("workshop"), "{listed}");
    assert!(listed.contains("just now"), "{listed}");

    // Never overwrite, never touch `empty`.
    for args in [
        &["state", "cp", "kp", "default", "workshop"][..],
        &["state", "mv", "kp", "default", "workshop"],
        &["state", "rm", "kp", "empty"],
        &["state", "rm", "kp", "missing"],
        &["state", "list", "nope"],
    ] {
        assert!(!koje(tmp.path(), args).status.success(), "{args:?}");
    }

    let out = stdout(&koje(tmp.path(), &["state", "mv", "kp", "default", "old"]));
    assert!(out.contains("`default` role still names default"), "{out}");
    let out = stdout(&koje(tmp.path(), &["state", "rm", "kp", "old"]));
    assert!(!out.contains("note"), "{out}");
    stdout(&koje(
        tmp.path(),
        &["state", "cp", "kp", "workshop", "default"],
    ));
    let out = stdout(&koje(tmp.path(), &["state", "rm", "kp", "default"]));
    assert!(out.contains("now loads fresh"), "{out}");
}

#[test]
fn list_shows_details_and_json() {
    let tmp = tempfile::tempdir().unwrap();
    stdout(&koje(tmp.path(), &["new", "kp"]));
    stdout(&koje(
        tmp.path(),
        &["state", "cp", "kp", "empty", "default"],
    ));
    let broken = config_of(tmp.path(), "zz");
    std::fs::create_dir_all(broken.parent().unwrap()).unwrap();
    std::fs::write(&broken, "name = 1\n").unwrap();

    let text = stdout(&koje(tmp.path(), &["list"]));
    assert!(text.contains("STATES"), "{text}");
    // Not a terminal: no escape codes.
    assert!(!text.contains('\x1b'), "{text:?}");
    let kp = text.lines().find(|l| l.starts_with("kp ")).unwrap();
    assert!(
        kp.contains("#2E7D5B") && kp.trim_end().ends_with('1'),
        "{kp}"
    );
    assert!(text.contains("zz "), "{text}");

    let json: serde_json::Value =
        serde_json::from_str(&stdout(&koje(tmp.path(), &["list", "--json"]))).unwrap();
    assert_eq!(json[0]["name"], "kp");
    assert_eq!(json[0]["prefix"], "KP");
    assert_eq!(json[0]["color"], "#2E7D5B");
    assert_eq!(json[0]["states"], 1);
    assert_eq!(json[0]["mounts"], serde_json::json!([]));
    assert!(json[0].get("error").is_none());
    assert_eq!(json[1]["name"], "zz");
    assert!(json[1]["error"].is_string());
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
