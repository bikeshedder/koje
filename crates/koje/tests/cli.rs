//! End-to-end tests of the `koje` binary with isolated XDG directories.
//! They never touch the real user configuration, Podman, Hyprland or kojed
//! (the session bus points nowhere).

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn command(xdg: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_koje"));
    cmd.args(args)
        .env("XDG_CONFIG_HOME", xdg.join("config"))
        .env("XDG_DATA_HOME", xdg.join("data"))
        .env("XDG_STATE_HOME", xdg.join("state"))
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            format!("unix:path={}", xdg.join("no-bus").display()),
        )
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

/// A fake `podman` on a private PATH: logs every call to `<tmp>/podman.log`
/// and keeps the container status in `<tmp>/podman.status` (create → exited,
/// start → running, stop → exited), like Podman for one container.
/// Written by a child process, so no fd of this test process points at the
/// script while other tests fork ("text file busy").
/// A fake `podman` on `PATH`. Contexts get no Wayland and no sound (global
/// config): both come from kojed (ADR 0022, 0029), which these tests do not run.
fn fake_podman(tmp: &Path) -> String {
    let global = tmp.join("config/koje/config.toml");
    std::fs::create_dir_all(global.parent().expect("config dir")).expect("config dir");
    std::fs::write(&global, "[access]\nwayland = false\nsound = false\n").expect("global config");
    let bin = tmp.join("bin");
    std::fs::create_dir_all(&bin).expect("bin dir");
    let (log, status) = (tmp.join("podman.log"), tmp.join("podman.status"));
    let script = format!(
        r#"#!/bin/sh
printf '%s\n' "$*" >> {log}
case "$1" in
  container) if [ -f {status} ]; then cat {status}; else echo "Error: no such container" >&2; exit 125; fi ;;
  image) exit 0 ;;
  volume) case "$2" in exists) test -f {status}.volume ;; rm) rm -f {status}.volume ;; esac ;;
  rm) rm -f {status} ;;
  create) echo exited > {status}; touch {status}.volume ;;
  start) echo running > {status} ;;
  stop) echo exited > {status} ;;
  pause) echo paused > {status} ;;
  unpause) echo running > {status} ;;
  exec) case "$*" in
    *"podman ps"*) echo web ;;
    *koje-forward*) touch {status}.forward ;;
    *ListNames*) if [ -f {status}.forward ]; then echo '"org.freedesktop.Notifications" "org.freedesktop.portal.Desktop"'; fi ;;
  esac ;;
esac
"#,
        log = log.display(),
        status = status.display()
    );
    write_tool(&bin, "podman", &script);
    format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

/// Write an executable script `bin/<name>` from a child process, so no fd of
/// this test process points at it while other tests fork ("text file busy").
fn write_tool(bin: &Path, name: &str, script: &str) {
    let ok = Command::new("sh")
        .args([
            "-c",
            &format!("cat > {0}/{name} && chmod +x {0}/{name}", bin.display()),
        ])
        .stdin(Stdio::piped())
        .spawn()
        .and_then(|mut c| {
            c.stdin
                .take()
                .expect("stdin")
                .write_all(script.as_bytes())?;
            c.wait()
        })
        .expect("write fake tool")
        .success();
    assert!(ok, "fake {name}");
}

fn podman_log(tmp: &Path) -> String {
    std::fs::read_to_string(tmp.join("podman.log")).unwrap_or_default()
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
fn image_build_runs_podman_build_for_the_user() {
    let tmp = tempfile::tempdir().unwrap();
    let path = fake_podman(tmp.path());
    let out = command(tmp.path(), &["image", "build", "--no-cache"])
        .env("PATH", path)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let calls = podman_log(tmp.path());
    assert!(
        calls.starts_with("build --tag localhost/koje/base:latest --build-arg USER_NAME="),
        "{calls}"
    );
    assert!(calls.contains("--no-cache"), "{calls}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("built koje/base:latest"));
}

fn koje_fake(tmp: &Path, path: &str, args: &[&str]) -> Output {
    command(tmp, args)
        .env("PATH", path)
        .stdin(Stdio::null())
        .output()
        .expect("run koje")
}

#[test]
fn up_creates_once_then_starts_and_down_stops() {
    let tmp = tempfile::tempdir().unwrap();
    let path = fake_podman(tmp.path());
    stdout(&koje(tmp.path(), &["new", "kp"]));

    let out = stdout(&koje_fake(tmp.path(), &path, &["up", "kp", "-s", "empty"]));
    assert!(out.contains("started kp (container created)"), "{out}");
    let calls = podman_log(tmp.path());
    assert!(
        calls.contains("image exists localhost/koje/base:latest"),
        "{calls}"
    );
    assert!(
        calls.contains("create --name koje-kp --hostname kp --init --userns=keep-id"),
        "{calls}"
    );
    assert!(calls.contains("--label io.koje.context=kp"), "{calls}");
    assert!(calls.lines().any(|l| l == "start koje-kp"), "{calls}");

    let out = stdout(&koje_fake(tmp.path(), &path, &["up", "kp", "-s", "empty"]));
    assert!(out.contains("already running"), "{out}");

    let out = stdout(&koje_fake(tmp.path(), &path, &["down", "kp", "--no-save"]));
    assert!(out.contains("stopped kp"), "{out}");
    let out = stdout(&koje_fake(tmp.path(), &path, &["down", "kp", "--no-save"]));
    assert!(out.contains("not running"), "{out}");

    // The second start reuses the container: no second create.
    let out = stdout(&koje_fake(tmp.path(), &path, &["up", "kp", "-s", "empty"]));
    assert!(
        out.contains("started kp") && !out.contains("created"),
        "{out}"
    );
    assert_eq!(podman_log(tmp.path()).matches("create --name").count(), 1);
}

#[test]
fn shell_needs_a_running_context_and_execs_into_it() {
    let tmp = tempfile::tempdir().unwrap();
    let path = fake_podman(tmp.path());
    stdout(&koje(tmp.path(), &["new", "kp"]));
    let out = koje_fake(tmp.path(), &path, &["shell", "kp"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("koje up kp -s empty"));

    stdout(&koje_fake(tmp.path(), &path, &["up", "kp", "-s", "empty"]));
    stdout(&koje_fake(tmp.path(), &path, &["shell", "kp"]));
    // stdin is not a terminal in tests: no -t.
    assert!(
        podman_log(tmp.path()).contains("exec -i koje-kp sh -c command -v bash"),
        "{}",
        podman_log(tmp.path())
    );

    stdout(&koje_fake(tmp.path(), &path, &["shell", "kp", "--dev"]));
    let log = podman_log(tmp.path());
    assert!(
        log.contains("exec koje-kp podman ps --format {{.Names}}"),
        "{log}"
    );
    assert!(
        log.contains("exec -i koje-kp podman exec -i web sh -c"),
        "{log}"
    );
    let out = koje_fake(tmp.path(), &path, &["shell", "kp", "--dev", "db"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("running: web"));
}

#[test]
fn open_passes_urls_and_mounted_files_only() {
    let tmp = tempfile::tempdir().unwrap();
    let path = fake_podman(tmp.path());
    stdout(&koje(tmp.path(), &["new", "kp"]));
    // A mounted folder (an absolute path outside any protected dir) with a file.
    let shared = tmp.path().join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::write(shared.join("spec.pdf"), "x").unwrap();
    std::fs::write(tmp.path().join("elsewhere.txt"), "x").unwrap();
    let cfg = config_of(tmp.path(), "kp");
    let text = std::fs::read_to_string(&cfg).unwrap();
    std::fs::write(
        &cfg,
        text.replace(
            "mounts = []",
            &format!("mounts = [{:?}]", shared.display().to_string()),
        ),
    )
    .unwrap();
    stdout(&koje_fake(tmp.path(), &path, &["up", "kp", "-s", "empty"]));

    let out = stdout(&koje_fake(
        tmp.path(),
        &path,
        &["open", "kp", "https://example.org"],
    ));
    assert!(out.contains("opened https://example.org in kp"), "{out}");
    let file = shared.join("spec.pdf").display().to_string();
    stdout(&koje_fake(tmp.path(), &path, &["open", "kp", &file]));
    let log = podman_log(tmp.path());
    assert!(
        log.contains("exec -d koje-kp xdg-open https://example.org"),
        "{log}"
    );
    assert!(
        log.contains(&format!(
            "exec -d koje-kp xdg-open {}",
            std::fs::canonicalize(&file).unwrap().display()
        )),
        "{log}"
    );

    let elsewhere = tmp.path().join("elsewhere.txt").display().to_string();
    let out = koje_fake(tmp.path(), &path, &["open", "kp", &elsewhere]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("not visible in kp"));
    let out = koje_fake(tmp.path(), &path, &["open", "kp", "/does/not/exist"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("does not exist"));
}

#[test]
fn sleep_and_wake() {
    let tmp = tempfile::tempdir().unwrap();
    let path = fake_podman(tmp.path());
    stdout(&koje(tmp.path(), &["new", "kp"]));
    assert!(
        !koje_fake(tmp.path(), &path, &["sleep", "kp"])
            .status
            .success()
    );
    stdout(&koje_fake(tmp.path(), &path, &["up", "kp", "-s", "empty"]));
    assert!(stdout(&koje_fake(tmp.path(), &path, &["sleep", "kp"])).contains("kp is asleep"));
    assert!(stdout(&koje_fake(tmp.path(), &path, &["sleep", "kp"])).contains("already asleep"));
    // up on a paused context points to wake.
    let out = koje_fake(tmp.path(), &path, &["up", "kp", "-s", "empty"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("koje wake kp"));
    assert!(stdout(&koje_fake(tmp.path(), &path, &["wake", "kp"])).contains("kp is awake"));
    assert!(stdout(&koje_fake(tmp.path(), &path, &["wake", "kp"])).contains("not asleep"));
    let log = podman_log(tmp.path());
    assert!(
        log.lines().any(|l| l == "pause koje-kp") && log.lines().any(|l| l == "unpause koje-kp"),
        "{log}"
    );
}

#[test]
fn rm_removes_container_and_home_volume() {
    let tmp = tempfile::tempdir().unwrap();
    let path = fake_podman(tmp.path());
    stdout(&koje(tmp.path(), &["new", "kp"]));
    stdout(&koje_fake(tmp.path(), &path, &["up", "kp", "-s", "empty"]));

    // The question lists the container (running) and the home volume; "n" keeps all.
    let out = command(tmp.path(), &["rm", "kp"])
        .env("PATH", &path)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("container koje-kp (running – stopped first)"),
        "{text}"
    );
    assert!(text.contains("home volume koje-kp-home"), "{text}");
    assert!(!out.status.success());
    assert!(config_of(tmp.path(), "kp").exists());

    stdout(&koje_fake(tmp.path(), &path, &["rm", "kp", "--yes"]));
    let log = podman_log(tmp.path());
    let tail: Vec<&str> = log
        .lines()
        .filter(|l| !l.starts_with("container") && !l.starts_with("volume exists"))
        .collect();
    assert!(
        tail.ends_with(&[
            "stop --time 10 koje-kp",
            "rm koje-kp",
            "volume rm koje-kp-home"
        ]),
        "{log}"
    );
    assert!(!config_of(tmp.path(), "kp").exists());
}

#[test]
fn up_starts_proxy_and_forwarder_down_stops_the_proxy() {
    let tmp = tempfile::tempdir().unwrap();
    let path = fake_podman(tmp.path());
    let bin = tmp.path().join("bin");
    let log = tmp.path().join("systemd.log");
    write_tool(&bin, "xdg-dbus-proxy", "#!/bin/sh\n");
    // is-active: never active; systemd-run: log, and create the socket path (ends in /bus).
    write_tool(
        &bin,
        "systemctl",
        &format!(
            "#!/bin/sh\nprintf 'systemctl %s\\n' \"$*\" >> {0}\ncase \"$*\" in *is-active*) exit 1 ;; esac\n",
            log.display()
        ),
    );
    write_tool(
        &bin,
        "systemd-run",
        &format!(
            "#!/bin/sh\nprintf 'systemd-run %s\\n' \"$*\" >> {0}\nfor a; do case \"$a\" in /*/bus) : > \"$a\" ;; esac; done\n",
            log.display()
        ),
    );
    let forwarder = tmp.path().join("koje-forward");
    std::fs::write(&forwarder, "").unwrap();
    let run = tmp.path().join("run");
    std::fs::create_dir_all(&run).unwrap();
    stdout(&koje(tmp.path(), &["new", "kp"]));

    let koje_up = |args: &[&str]| {
        command(tmp.path(), args)
            .env("PATH", &path)
            .env("XDG_RUNTIME_DIR", &run)
            .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/fake/host/bus")
            .env("KOJE_FORWARD", &forwarder)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let out = stdout(&koje_up(&["up", "kp", "-s", "empty"]));
    assert!(
        !out.contains("host interfaces"),
        "no host interface note expected:\n{out}"
    );

    let systemd = std::fs::read_to_string(&log).unwrap();
    let dir = run.join("koje/kp/host-bus");
    assert!(
        systemd.contains("systemd-run --user --unit=koje-proxy-kp --collect"),
        "{systemd}"
    );
    assert!(
        systemd.contains(&format!(
            "xdg-dbus-proxy unix:path=/fake/host/bus {}/bus --filter",
            dir.display()
        )),
        "{systemd}"
    );
    assert!(
        systemd.contains("--talk=org.freedesktop.Notifications"),
        "{systemd}"
    );

    let podman = podman_log(tmp.path());
    assert!(
        podman.contains(&format!("--volume {}:/run/user/", dir.display())),
        "{podman}"
    );
    assert!(
        podman.contains(&format!(
            "--volume {}:/usr/local/libexec/koje/koje-forward:ro",
            forwarder.display()
        )),
        "{podman}"
    );
    assert!(
        podman.contains(
            "exec -d koje-kp /usr/local/libexec/koje/koje-forward --host unix:path=/run/user/"
        ) && podman.contains("org.freedesktop.Notifications org.freedesktop.portal.Desktop"),
        "{podman}"
    );

    // Again: the forwarder already owns its names and is not started twice.
    stdout(&koje_up(&["up", "kp", "-s", "empty"]));
    let starts = podman_log(tmp.path())
        .matches("exec -d koje-kp /usr/local/libexec/koje/koje-forward")
        .count();
    assert_eq!(starts, 1);

    stdout(&koje_up(&["down", "kp", "--no-save"]));
    assert!(
        std::fs::read_to_string(&log)
            .unwrap()
            .contains("systemctl --user stop koje-proxy-kp")
    );
}

#[test]
fn up_falls_back_to_empty_and_restores_through_kojed() {
    let tmp = tempfile::tempdir().unwrap();
    let path = fake_podman(tmp.path());
    stdout(&koje(tmp.path(), &["new", "kp"]));
    assert!(
        !koje_fake(tmp.path(), &path, &["up", "nope", "-s", "empty"])
            .status
            .success()
    );
    assert_eq!(podman_log(tmp.path()), "", "nothing reached podman");

    // No state yet: `default` → `fresh` → `empty`.
    let out = stdout(&koje_fake(tmp.path(), &path, &["up", "kp"]));
    assert!(
        out.contains("no state default yet – starting kp without windows"),
        "{out}"
    );
    let out = koje_fake(tmp.path(), &path, &["down", "kp"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("koje down kp --no-save"));
    let out = stdout(&koje_fake(tmp.path(), &path, &["down", "kp", "--no-save"]));
    assert!(
        out.contains("programs not closed in order") && out.contains("stopped kp"),
        "{out}"
    );
    assert!(podman_log(tmp.path()).contains("stop --time 10 koje-kp"));

    // A stored state with windows is restored by kojed (not running here).
    let states = tmp.path().join("data/koje/kp/states");
    std::fs::create_dir_all(&states).unwrap();
    std::fs::write(
        states.join("default.toml"),
        "active_layout = \"main\"\n[[windows]]\nid = \"term\"\ncommand = \"foot\"\n",
    )
    .unwrap();
    let out = koje_fake(tmp.path(), &path, &["up", "kp"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("restoring state default of kp"), "{err}");
}

#[test]
fn unimplemented_commands_name_their_milestone() {
    let tmp = tempfile::tempdir().unwrap();
    let out = koje(tmp.path(), &["stats"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("milestone M6"));
}

#[test]
fn up_with_wayland_needs_kojed() {
    let tmp = tempfile::tempdir().unwrap();
    let path = fake_podman(tmp.path());
    std::fs::remove_file(tmp.path().join("config/koje/config.toml")).unwrap();
    stdout(&koje(tmp.path(), &["new", "kp"]));
    let out = command(tmp.path(), &["up", "kp", "-s", "empty"])
        .env("PATH", &path)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("Wayland socket"), "{err}");
    // Nothing was created without the socket.
    assert!(
        !podman_log(tmp.path()).contains("create"),
        "{}",
        podman_log(tmp.path())
    );
}

#[test]
fn save_needs_a_running_context_and_a_writable_state() {
    let tmp = tempfile::tempdir().unwrap();
    let path = fake_podman(tmp.path());
    stdout(&koje(tmp.path(), &["new", "kp"]));
    let save = |args: &[&str]| {
        command(tmp.path(), args)
            .env("PATH", &path)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let out = save(&["save", "kp", "-s", "empty"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("empty"));
    // Not created yet (fake podman: no such container): nothing to save.
    let out = save(&["save", "kp"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("not running"), "{err}");
    assert!(!tmp.path().join("data/koje/kp/states/default.toml").exists());
}

#[test]
fn focus_needs_the_context_and_kojed() {
    let tmp = tempfile::tempdir().unwrap();
    let out = koje(tmp.path(), &["focus", "kp"]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("kp"),
        "unknown context named"
    );
    assert!(koje(tmp.path(), &["new", "kp"]).status.success());
    // The test's session bus points nowhere.
    let out = koje(tmp.path(), &["focus", "kp"]);
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("session bus"), "{err}");
}

#[test]
fn usage_errors_exit_with_code_2() {
    let tmp = tempfile::tempdir().unwrap();
    let out = koje(tmp.path(), &["down", "kp", "-s", "x", "--no-save"]);
    assert_eq!(out.status.code(), Some(2));
}

/// `koje setup` in a temporary home (fake podman: the image exists).
fn setup_cmd(tmp: &Path, path: &str, args: &[&str]) -> Output {
    let mut all = vec!["setup"];
    all.extend_from_slice(args);
    command(tmp, &all)
        .env("HOME", tmp)
        .env("PATH", path)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn setup_installs_then_has_nothing_to_do() {
    let tmp = tempfile::tempdir().unwrap();
    let t = tmp.path();
    let path = fake_podman(t);

    let out = stdout(&setup_cmd(t, &path, &["--dry-run"]));
    assert!(out.contains("+ Hyprland snippet"), "{out}");
    assert!(out.contains("dry run – nothing changed"), "{out}");
    assert!(!t.join("data/koje/hyprland/koje.conf").exists());

    let out = stdout(&setup_cmd(t, &path, &["--yes"]));
    assert!(out.contains("done: Hyprland snippet"), "{out}");
    let snippet = std::fs::read_to_string(t.join("data/koje/hyprland/koje.conf")).unwrap();
    assert!(snippet.contains("exec-once = systemctl --user start kojed"));
    assert!(t.join("data/fonts/koje/lucide.ttf").exists());
    assert!(t.join("config/systemd/user/kojed.service").exists());
    let picker = t.join(".local/lib/koje/koje-window-picker");
    assert_eq!(
        std::fs::metadata(&picker).unwrap().permissions().mode() & 0o777,
        0o755
    );
    let conf = std::fs::read_to_string(t.join("config/hypr/hyprland.conf")).unwrap();
    assert!(
        conf.contains(&format!(
            "source = {}",
            t.join("data/koje/hyprland/koje.conf").display()
        )),
        "{conf}"
    );
    assert!(t.join("config/waybar/config.jsonc").exists());

    let out = stdout(&setup_cmd(t, &path, &["--yes"]));
    assert!(out.contains("nothing to do"), "{out}");
}

#[test]
fn setup_leaves_an_own_waybar_and_appends_once() {
    let tmp = tempfile::tempdir().unwrap();
    let t = tmp.path();
    let path = fake_podman(t);
    std::fs::create_dir_all(t.join("config/waybar")).unwrap();
    std::fs::write(t.join("config/waybar/config"), "{ \"layer\": \"top\" }").unwrap();
    std::fs::create_dir_all(t.join("config/hypr")).unwrap();
    std::fs::write(
        t.join("config/hypr/hyprland.conf"),
        "monitor = ,preferred,auto,1\n",
    )
    .unwrap();

    let out = stdout(&setup_cmd(t, &path, &["--yes"]));
    assert!(out.contains("- Waybar: your own config"), "{out}");
    assert!(out.contains("~ Hyprland config"), "{out}");
    assert!(!t.join("config/waybar/config.jsonc").exists());
    let conf = std::fs::read_to_string(t.join("config/hypr/hyprland.conf")).unwrap();
    assert!(
        conf.starts_with("monitor = ,preferred,auto,1\n"),
        "never rewritten: {conf}"
    );
    stdout(&setup_cmd(t, &path, &["--yes"]));
    let again = std::fs::read_to_string(t.join("config/hypr/hyprland.conf")).unwrap();
    assert_eq!(again.matches("source = ").count(), 1, "{again}");
}

#[test]
fn setup_asks_and_stdin_closed_means_no() {
    let tmp = tempfile::tempdir().unwrap();
    let t = tmp.path();
    let path = fake_podman(t);
    let out = setup_cmd(t, &path, &[]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("nothing changed"));
    assert!(!t.join("data/koje/hyprland/koje.conf").exists());
}
