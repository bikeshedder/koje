//! End-to-end tests of the `koje` binary with isolated XDG directories.
//! They never touch the real user configuration, Podman, GNOME or the
//! user's kojed: the session bus points nowhere, or to a private bus with a
//! kojed of the test's own ([`Session`]).

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn command(xdg: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_koje"));
    cmd.args(args)
        .env("XDG_CONFIG_HOME", xdg.join("config"))
        .env("XDG_DATA_HOME", xdg.join("data"))
        .env("XDG_STATE_HOME", xdg.join("state"))
        .env("XDG_RUNTIME_DIR", xdg.join("run"))
        // A home of its own: mounts must be below it (ADR 0034).
        .env("HOME", xdg)
        .env(
            "DBUS_SESSION_BUS_ADDRESS",
            format!("unix:path={}", xdg.join("no-bus").display()),
        )
        .env_remove("RUST_LOG")
        .env_remove("VISUAL")
        .env_remove("EDITOR")
        // Hyprland unless a test says otherwise, also when run in GNOME.
        .env_remove("KOJE_COMPOSITOR")
        .env_remove("XDG_CURRENT_DESKTOP");
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
  container) if [ ! -f {status} ]; then echo "Error: no such container" >&2; exit 125; fi
    case "$*" in *io.koje.config*) cat {status}.fp 2>/dev/null || echo "<no value>" ;; *Labels*) echo "<no value>" ;; *.Image*) echo sha256:fake ;; *) cat {status} ;; esac ;;
  image) case "$*" in *inspect*.Id*) echo sha256:fake ;; esac ;;
  volume) case "$2" in exists) test -f {status}.volume ;; rm) rm -f {status}.volume ;; create) touch {status}.volume ;; inspect) echo {status}.voldata ;; esac ;;
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
    assert!(text.contains("image  = \"fedora\""), "{text}");

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
fn edit_from_stdin_checks_like_the_editor() {
    let tmp = tempfile::tempdir().unwrap();
    stdout(&koje(tmp.path(), &["new", "kp"]));
    let path = config_of(tmp.path(), "kp");
    let text = std::fs::read_to_string(&path).unwrap();
    let changed = text.replace("prefix = \"KP\"", "prefix = \"XY\"");
    let out = koje_with(tmp.path(), &["edit", "kp", "--stdin"], &[], &changed);
    assert!(stdout(&out).contains("saved"));
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("prefix = \"XY\"")
    );
    // Invalid: refused, the file stays.
    let bad = koje_with(
        tmp.path(),
        &["edit", "kp", "--stdin"],
        &[],
        "name = \"other\"\n",
    );
    assert!(!bad.status.success());
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("prefix = \"XY\"")
    );
}

#[test]
fn config_from_stdin_is_checked() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config/koje/config.toml");
    let good = "[default_limits]\nmemory_soft = \"4G\"\n";
    let out = koje_with(tmp.path(), &["config", "--stdin"], &[], good);
    assert!(stdout(&out).contains("saved"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), good);
    // Invalid: refused, the file stays.
    let bad = koje_with(
        tmp.path(),
        &["config", "--stdin"],
        &[],
        "[default_limits]\nmemory_soft = \"lots\"\n",
    );
    assert!(!bad.status.success());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), good);
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
fn list_shows_details() {
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
    // koje's image `fedora` (ADR 0040), then a look into it, then the user layer.
    let image = calls
        .lines()
        .find(|l| l.starts_with("build --tag localhost/koje/fedora:latest "))
        .unwrap_or_else(|| panic!("{calls}"));
    assert!(image.contains("--label io.koje.source="), "{image}");
    assert!(image.contains("--no-cache"), "{image}");
    assert!(
        calls.contains("run --rm --user root --network none --pull never --entrypoint sh localhost/koje/fedora:latest -c"),
        "{calls}"
    );
    let layer = calls
        .lines()
        .find(|l| l.starts_with("build --tag localhost/koje/user/fedora:latest "))
        .unwrap_or_else(|| panic!("{calls}"));
    assert!(
        layer.contains("--build-arg KOJE_IMAGE=localhost/koje/fedora:latest"),
        "{layer}"
    );
    assert!(layer.contains("--build-arg USER_NAME="), "{layer}");
    assert!(
        layer.contains("--label io.koje.gbm-dir=/usr/lib64/gbm"),
        "{layer}"
    );
    assert!(layer.contains("--pull=never"), "{layer}");
    assert!(
        String::from_utf8_lossy(&out.stdout)
            .contains("built fedora → localhost/koje/user/fedora:latest")
    );
}

/// A private session bus with kojed on it (G.29: kojed runs `up`, `down`,
/// `sleep`, `recreate`, `save`), with the test's XDG directories and `PATH`;
/// killed on drop. kojed waits for GNOME Shell and serves meanwhile.
struct Session {
    tmp: PathBuf,
    path: String,
    bus: String,
    bus_pid: u32,
    kojed: std::process::Child,
}

impl Session {
    fn start(tmp: &Path, path: &str) -> Self {
        Self::start_with(tmp, path, &[])
    }

    fn start_with(tmp: &Path, path: &str, env: &[(&str, &str)]) -> Self {
        let out = Command::new("dbus-daemon")
            .args(["--session", "--fork", "--print-address=1", "--print-pid=2"])
            .output()
            .expect("dbus-daemon (the dev container has it)");
        let bus = String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .expect("bus address")
            .to_owned();
        let bus_pid = String::from_utf8_lossy(&out.stderr).trim().parse().unwrap();
        // Next to koje: `cargo test --workspace` builds both.
        let exe = Path::new(env!("CARGO_BIN_EXE_koje")).with_file_name("kojed");
        assert!(exe.exists(), "build kojed first: cargo build -p kojed");
        std::fs::create_dir_all(tmp.join("run")).unwrap();
        let kojed = command(tmp, &[])
            .get_envs()
            .fold(Command::new(exe), |mut c, (k, v)| {
                match v {
                    Some(v) => c.env(k, v),
                    None => c.env_remove(k),
                };
                c
            })
            .env("PATH", path)
            .env("DBUS_SESSION_BUS_ADDRESS", &bus)
            .envs(env.iter().copied())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start kojed");
        let session = Self {
            tmp: tmp.to_owned(),
            path: path.to_owned(),
            bus,
            bus_pid,
            kojed,
        };
        for _ in 0..100 {
            let owned = Command::new("dbus-send")
                .args([
                    &format!("--bus={}", session.bus),
                    "--print-reply",
                    "--dest=org.freedesktop.DBus",
                    "/org/freedesktop/DBus",
                    "org.freedesktop.DBus.NameHasOwner",
                    "string:io.koje.Daemon",
                ])
                .output()
                .unwrap();
            if String::from_utf8_lossy(&owned.stdout).contains("boolean true") {
                return session;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("kojed did not take io.koje.Daemon");
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = command(&self.tmp, args);
        cmd.env("PATH", &self.path)
            .env("DBUS_SESSION_BUS_ADDRESS", &self.bus)
            .stdin(Stdio::null());
        cmd
    }

    fn koje(&self, args: &[&str]) -> Output {
        self.command(args).output().expect("run koje")
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.kojed.kill();
        let _ = self.kojed.wait();
        let _ = Command::new("kill").arg(self.bus_pid.to_string()).status();
    }
}

fn koje_fake(tmp: &Path, path: &str, args: &[&str]) -> Output {
    command(tmp, args)
        .env("PATH", path)
        .stdin(Stdio::null())
        .output()
        .expect("run koje")
}

#[test]
fn image_new_creates_a_folder_building_on_its_image() {
    let tmp = tempfile::tempdir().unwrap();
    let out = stdout(&koje(tmp.path(), &["image", "new", "dev"]));
    let file = tmp.path().join("config/koje/images/dev/Containerfile");
    assert!(out.contains(&file.display().to_string()), "{out}");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(
        text.ends_with("FROM localhost/koje/fedora:latest\n"),
        "{text}"
    );
    assert!(!koje(tmp.path(), &["image", "new", "dev"]).status.success());

    stdout(&koje(tmp.path(), &["image", "new", "web", "--from", "dev"]));
    let web =
        std::fs::read_to_string(tmp.path().join("config/koje/images/web/Containerfile")).unwrap();
    assert!(web.ends_with("FROM localhost/koje/dev:latest\n"), "{web}");
    let reg = koje(
        tmp.path(),
        &[
            "image",
            "new",
            "deb",
            "--from",
            "docker.io/library/debian:13",
        ],
    );
    assert!(reg.status.success());
    let deb =
        std::fs::read_to_string(tmp.path().join("config/koje/images/deb/Containerfile")).unwrap();
    assert!(deb.ends_with("FROM docker.io/library/debian:13\n"), "{deb}");
    // An image folder that does not exist cannot be built on.
    assert!(
        !koje(tmp.path(), &["image", "new", "x", "--from", "nope"])
            .status
            .success()
    );
    // Listed like contexts (ADR 0064), with a label and the contexts using it.
    stdout(&koje(
        tmp.path(),
        &["image", "new", "app", "--label", "App work"],
    ));
    stdout(&koje(tmp.path(), &["new", "kp"]));
    let config = config_of(tmp.path(), "kp");
    let text = std::fs::read_to_string(&config).unwrap();
    std::fs::write(
        &config,
        text.replace("image  = \"fedora\"", "image  = \"app\""),
    )
    .unwrap();
    let list = stdout(&koje(tmp.path(), &["image", "list"]));
    let line = |name: &str| {
        list.lines()
            .find(|l| l.starts_with(&format!("{name} ")))
            .unwrap_or_else(|| panic!("{list}"))
            .to_owned()
    };
    assert!(line("fedora").contains("Fedora (koje's)"), "{list}");
    assert!(
        line("app").contains("App work") && line("app").trim_end().ends_with("kp"),
        "{list}"
    );
    assert!(line("dev").trim_end().ends_with('-'), "{list}");
    // Removed only when no context uses it.
    let used = koje(tmp.path(), &["image", "rm", "app", "--yes"]);
    assert!(!used.status.success());
    assert!(String::from_utf8_lossy(&used.stderr).contains("used by kp"));
    stdout(&koje(tmp.path(), &["image", "rm", "web", "--yes"]));
    assert!(!tmp.path().join("config/koje/images/web").exists());
    // koje's own image, customized and reset (ADR 0065).
    stdout(&koje(tmp.path(), &["image", "customize", "fedora"]));
    let file = tmp.path().join("config/koje/images/fedora/Containerfile");
    assert!(
        std::fs::read_to_string(&file)
            .unwrap()
            .contains("FROM registry.fedoraproject.org/fedora")
    );
    let list = stdout(&koje(tmp.path(), &["image", "list"]));
    assert!(list.contains("Fedora (customized)"), "{list}");
    assert!(
        !koje(tmp.path(), &["image", "customize", "app"])
            .status
            .success()
    );
    stdout(&koje(tmp.path(), &["image", "reset", "fedora", "--yes"]));
    assert!(!file.exists());
}

#[test]
fn image_edit_makes_the_contexts_own_and_opens_it() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let opened = tmp.path().join("opened");
    write_tool(
        &bin,
        "xdg-open",
        &format!("#!/bin/sh\nprintf '%s\\n' \"$1\" >> {}\n", opened.display()),
    );
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    stdout(&koje(tmp.path(), &["new", "kp"]));

    let out = stdout(&koje_fake(tmp.path(), &path, &["image", "edit", "kp"]));
    let file = tmp.path().join("config/koje/images/kp/Containerfile");
    assert!(
        out.contains(&format!("created {}", file.display())),
        "{out}"
    );
    assert!(out.contains("kp now uses image \"kp\""), "{out}");
    assert_eq!(
        std::fs::read_to_string(&opened).unwrap(),
        format!("{}\n", file.display())
    );
    let text = std::fs::read_to_string(config_of(tmp.path(), "kp")).unwrap();
    assert!(text.contains("image  = \"kp\""), "{text}");

    // Again: the same file, opened, not made anew.
    let out = stdout(&koje_fake(tmp.path(), &path, &["image", "edit", "kp"]));
    assert!(!out.contains("created /"), "{out}");
    assert_eq!(std::fs::read_to_string(&opened).unwrap().lines().count(), 2);
}

#[test]
fn up_builds_an_image_folder_after_what_it_builds_on() {
    let tmp = tempfile::tempdir().unwrap();
    let path = fake_podman(tmp.path());
    let s = Session::start(tmp.path(), &path);
    stdout(&koje(tmp.path(), &["new", "kp"]));
    stdout(&koje(tmp.path(), &["image", "new", "dev"]));
    let config = config_of(tmp.path(), "kp");
    let text = std::fs::read_to_string(&config).unwrap();
    std::fs::write(
        &config,
        text.replace("image  = \"fedora\"", "image  = \"dev\""),
    )
    .unwrap();

    let out = stdout(&s.koje(&["up", "kp", "-s", "empty"]));
    assert!(out.contains("started kp (container created)"), "{out}");
    let calls = podman_log(tmp.path());
    let builds: Vec<&str> = calls
        .lines()
        .filter_map(|l| l.strip_prefix("build --tag "))
        .map(|l| l.split(' ').next().unwrap_or_default())
        .collect();
    assert_eq!(
        builds,
        [
            "localhost/koje/fedora:latest",
            "localhost/koje/dev:latest",
            "localhost/koje/user/dev:latest"
        ],
        "{calls}"
    );
    let dev = tmp.path().join("config/koje/images/dev");
    assert!(
        calls.contains(&format!("--file {}/Containerfile", dev.display())),
        "{calls}"
    );
    assert!(
        calls.contains(" localhost/koje/user/dev:latest dbus-daemon"),
        "{calls}"
    );
}

#[test]
fn up_creates_once_then_starts_and_down_stops() {
    let tmp = tempfile::tempdir().unwrap();
    let path = fake_podman(tmp.path());
    let s = Session::start(tmp.path(), &path);
    stdout(&koje(tmp.path(), &["new", "kp"]));

    let out = stdout(&s.koje(&["up", "kp", "-s", "empty"]));
    assert!(out.contains("started kp (container created)"), "{out}");
    let calls = podman_log(tmp.path());
    // The first `up` builds koje's image with your user and runs it.
    assert!(
        calls.contains("build --tag localhost/koje/user/fedora:latest "),
        "{calls}"
    );
    assert!(
        calls.contains(" localhost/koje/user/fedora:latest dbus-daemon --session"),
        "{calls}"
    );
    assert!(
        calls.contains("create --name koje-kp --hostname kp --init --userns=keep-id"),
        "{calls}"
    );
    assert!(calls.contains("--label io.koje.context=kp"), "{calls}");
    assert!(calls.lines().any(|l| l == "start koje-kp"), "{calls}");
    // Passwordless sudo for the user (ADR 0066), as root, the name as an argument.
    assert!(
        calls.contains("exec --user root koje-kp sh -c set -e"),
        "{calls}"
    );
    assert!(calls.contains("NOPASSWD: ALL"), "{calls}");

    // Without -s: a running context stays as it is (-s switches its state).
    let out = stdout(&s.koje(&["up", "kp"]));
    assert!(out.contains("already running"), "{out}");

    let out = stdout(&s.koje(&["down", "kp", "--no-save"]));
    assert!(out.contains("stopped kp"), "{out}");
    let out = stdout(&s.koje(&["down", "kp", "--no-save"]));
    assert!(out.contains("not running"), "{out}");

    // The second start reuses the container: no second create.
    let out = stdout(&s.koje(&["up", "kp", "-s", "empty"]));
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
    let s = Session::start(tmp.path(), &path);
    stdout(&koje(tmp.path(), &["new", "kp"]));
    let out = s.koje(&["shell", "kp"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("koje up kp -s empty"));

    stdout(&s.koje(&["up", "kp", "-s", "empty"]));
    stdout(&s.koje(&["shell", "kp"]));
    // stdin is not a terminal in tests: no -t.
    assert!(
        podman_log(tmp.path()).contains("exec -i koje-kp sh -c command -v bash"),
        "{}",
        podman_log(tmp.path())
    );

    stdout(&s.koje(&["shell", "kp", "--dev"]));
    let log = podman_log(tmp.path());
    assert!(
        log.contains("exec koje-kp podman ps --format {{.Names}}"),
        "{log}"
    );
    assert!(
        log.contains("exec -i koje-kp podman exec -i web sh -c"),
        "{log}"
    );
    let out = s.koje(&["shell", "kp", "--dev", "db"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("running: web"));
}

#[test]
fn open_passes_urls_and_mounted_files_only() {
    let tmp = tempfile::tempdir().unwrap();
    let path = fake_podman(tmp.path());
    let s = Session::start(tmp.path(), &path);
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
    stdout(&s.koje(&["up", "kp", "-s", "empty"]));

    let out = stdout(&s.koje(&["open", "kp", "https://example.org"]));
    assert!(out.contains("opened https://example.org in kp"), "{out}");
    let file = shared.join("spec.pdf").display().to_string();
    stdout(&s.koje(&["open", "kp", &file]));
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
    let out = s.koje(&["open", "kp", &elsewhere]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("not visible in kp"));
    let out = s.koje(&["open", "kp", "/does/not/exist"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("does not exist"));
}

#[test]
fn sleep_and_wake() {
    let tmp = tempfile::tempdir().unwrap();
    let path = fake_podman(tmp.path());
    let s = Session::start(tmp.path(), &path);
    stdout(&koje(tmp.path(), &["new", "kp"]));
    assert!(!s.koje(&["sleep", "kp"]).status.success());
    stdout(&s.koje(&["up", "kp", "-s", "empty"]));
    assert!(stdout(&s.koje(&["sleep", "kp"])).contains("kp is asleep"));
    assert!(stdout(&s.koje(&["sleep", "kp"])).contains("already asleep"));
    // up on a paused context points to wake.
    let out = s.koje(&["up", "kp", "-s", "empty"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("koje wake kp"));
    assert!(stdout(&s.koje(&["wake", "kp"])).contains("kp is awake"));
    assert!(stdout(&s.koje(&["wake", "kp"])).contains("not asleep"));
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
    let s = Session::start(tmp.path(), &path);
    stdout(&koje(tmp.path(), &["new", "kp"]));
    stdout(&s.koje(&["up", "kp", "-s", "empty"]));

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

    stdout(&s.koje(&["rm", "kp", "--yes"]));
    let log = podman_log(tmp.path());
    let tail: Vec<&str> = log
        .lines()
        .filter(|l| {
            !l.starts_with("container")
                && !l.starts_with("volume exists")
                && !l.starts_with("volume inspect")
                && !l.starts_with("unshare")
        })
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
    stdout(&koje(tmp.path(), &["new", "kp"]));
    // kojed runs `up`: the forwarder and the host bus are kojed's.
    let s = Session::start_with(
        tmp.path(),
        &path,
        &[("KOJE_FORWARD", forwarder.to_str().unwrap())],
    );
    let koje_up = |args: &[&str]| s.koje(args);
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
            "xdg-dbus-proxy {} {}/bus --filter",
            s.bus,
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
    stdout(&koje_up(&["up", "kp"]));
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
    let s = Session::start(tmp.path(), &path);
    stdout(&koje(tmp.path(), &["new", "kp"]));
    assert!(!s.koje(&["up", "nope", "-s", "empty"]).status.success());
    assert_eq!(podman_log(tmp.path()), "", "nothing reached podman");

    // No state yet: `default` → `fresh` → `empty`.
    let out = stdout(&s.koje(&["up", "kp"]));
    assert!(
        out.contains("no state default yet – starting kp without windows"),
        "{out}"
    );
    let out = stdout(&s.koje(&["down", "kp", "--no-save"]));
    assert!(
        out.contains("programs not closed in order") && out.contains("stopped kp"),
        "{out}"
    );
    assert!(podman_log(tmp.path()).contains("stop --time 10 koje-kp"));

    // A stored state with windows: kojed starts its programs.
    let states = tmp.path().join("data/koje/kp/states");
    std::fs::create_dir_all(&states).unwrap();
    std::fs::write(
        states.join("default.toml"),
        "active_layout = \"main\"\n[[windows]]\nid = \"term\"\ncommand = \"foot\"\n",
    )
    .unwrap();
    let out = stdout(&s.koje(&["up", "kp"]));
    assert!(
        out.contains("restoring state default: 1 window(s), 1 program(s) started"),
        "{out}"
    );
}

#[test]
fn up_with_wayland_needs_the_session_socket() {
    let tmp = tempfile::tempdir().unwrap();
    let path = fake_podman(tmp.path());
    std::fs::remove_file(tmp.path().join("config/koje/config.toml")).unwrap();
    stdout(&koje(tmp.path(), &["new", "kp"]));
    // No kojed: nothing to run it.
    let out = koje_fake(tmp.path(), &path, &["up", "kp", "-s", "empty"]);
    assert!(!out.status.success());
    // kojed, but no GNOME session with its Wayland socket.
    let s = Session::start(tmp.path(), &path);
    let out = s.koje(&["up", "kp", "-s", "empty"]);
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
    let s = Session::start(tmp.path(), &path);
    stdout(&koje(tmp.path(), &["new", "kp"]));
    let save = |args: &[&str]| s.koje(args);
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
    setup_with_kojed(tmp, path, args, 0)
}

/// [`setup_cmd`] with a fake `systemctl` that logs its calls to
/// `systemctl.log` and reports `kojed_pid` as kojed's main process (0: not
/// running), and fake `gnome-extensions` and `gsettings` that remember the
/// enabled extension in `extension.on` – never the real user services or
/// GNOME settings.
fn setup_with_kojed(tmp: &Path, path: &str, args: &[&str], kojed_pid: u32) -> Output {
    let log = tmp.join("systemctl.log");
    let bin = tmp.join("bin");
    write_tool(
        &bin,
        "systemctl",
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\ncase \"$*\" in *MainPID*) echo {kojed_pid} ;; esac\n",
            log.display()
        ),
    );
    let on = tmp.join("extension.on");
    write_tool(
        &bin,
        "gnome-extensions",
        &format!("#!/bin/sh\n[ \"$1\" = enable ] && touch {}\n", on.display()),
    );
    write_tool(
        &bin,
        "gsettings",
        &format!(
            "#!/bin/sh\nif [ -f {0} ]; then echo \"['koje@koje.io']\"; else echo '@as []'; fi\n",
            on.display()
        ),
    );
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
    assert!(
        out.contains("+ GNOME Shell extension koje@koje.io: enable"),
        "{out}"
    );
    assert!(out.contains("= image: fedora"), "{out}");
    assert!(out.contains("dry run – nothing changed"), "{out}");
    assert!(!t.join("extension.on").exists());

    let out = stdout(&setup_cmd(t, &path, &["--yes"]));
    assert!(out.contains("done: GNOME Shell extension"), "{out}");
    assert!(t.join("extension.on").exists());

    let out = stdout(&setup_cmd(t, &path, &["--yes"]));
    assert!(out.contains("nothing to do"), "{out}");
}

#[test]
fn setup_restarts_a_kojed_with_an_old_binary() {
    let tmp = tempfile::tempdir().unwrap();
    let t = tmp.path();
    let path = fake_podman(t);
    stdout(&setup_cmd(t, &path, &["--yes"]));
    // This test's own process stands in for a kojed outside /usr/bin.
    let out = stdout(&setup_with_kojed(t, &path, &["--yes"], std::process::id()));
    assert!(
        out.contains("restart kojed (it runs an old binary)"),
        "{out}"
    );
    let log = std::fs::read_to_string(t.join("systemctl.log")).unwrap();
    assert!(log.contains("--user restart kojed"), "{log}");
}

#[test]
fn setup_asks_and_stdin_closed_means_no() {
    let tmp = tempfile::tempdir().unwrap();
    let t = tmp.path();
    let path = fake_podman(t);
    let out = setup_cmd(t, &path, &[]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("nothing changed"));
    assert!(!t.join("extension.on").exists());
}

#[test]
fn run_starts_a_program_in_the_running_context() {
    let tmp = tempfile::tempdir().unwrap();
    let path = fake_podman(tmp.path());
    let s = Session::start(tmp.path(), &path);
    stdout(&koje(tmp.path(), &["new", "kp"]));
    let shared = tmp.path().join("shared");
    std::fs::create_dir_all(&shared).unwrap();
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
    let run = |dir: &Path, args: &[&str]| {
        command(tmp.path(), args)
            .env("PATH", &path)
            .current_dir(dir)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let out = run(tmp.path(), &["run", "kp", "foot"]);
    assert!(!out.status.success(), "not running yet");
    assert!(String::from_utf8_lossy(&out.stderr).contains("koje up kp"));

    stdout(&s.koje(&["up", "kp", "-s", "empty"]));
    stdout(&run(&shared, &["run", "kp", "foot", "--app-id=x y"]));
    stdout(&run(tmp.path(), &["run", "kp", "firefox"]));
    let log = podman_log(tmp.path());
    let shared = std::fs::canonicalize(&shared).unwrap();
    assert!(
        log.contains(&format!(
            "exec -d --workdir {} koje-kp sh -c exec foot '--app-id=x y'",
            shared.display()
        )),
        "in the mounted folder: {log}"
    );
    assert!(
        log.contains("exec -d koje-kp sh -c exec firefox"),
        "elsewhere: home: {log}"
    );
}

#[test]
fn up_after_a_crash_keeps_or_takes_the_recovery_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = fake_podman(tmp.path());
    let s = Session::start(tmp.path(), &path);
    stdout(&koje(tmp.path(), &["new", "kp"]));
    let recovery = tmp.path().join("state/koje/kp/recovery.toml");
    std::fs::create_dir_all(recovery.parent().unwrap()).unwrap();
    std::fs::write(
        &recovery,
        "active_layout = \"main\"\n[[windows]]\nid = \"term\"\ncommand = \"foot\"\n",
    )
    .unwrap();
    // No terminal to ask: kept, the normal state (none: empty) is loaded.
    let out = stdout(&s.koje(&["up", "kp"]));
    assert!(out.contains("recovery file is kept"), "{out}");
    assert!(recovery.exists());
    stdout(&s.koje(&["down", "kp", "--no-save"]));
    assert!(!recovery.exists(), "a clean down deletes it");

    // --recover takes it.
    std::fs::write(
        &recovery,
        "active_layout = \"main\"\n[[windows]]\nid = \"term\"\ncommand = \"foot\"\n",
    )
    .unwrap();
    let out = stdout(&s.koje(&["up", "kp", "--recover"]));
    assert!(
        out.contains("restoring the recovery file: 1 window(s)"),
        "{out}"
    );
    // --no-recover discards it.
    stdout(&s.koje(&["down", "kp", "--no-save"]));
    std::fs::write(&recovery, "active_layout = \"main\"\n").unwrap();
    let out = stdout(&s.koje(&["up", "kp", "--no-recover"]));
    assert!(out.contains("discarded the recovery file"), "{out}");
    assert!(!recovery.exists());
}

#[test]
fn up_prepares_live_mounts() {
    let tmp = tempfile::tempdir().unwrap();
    let path = fake_podman(tmp.path());
    let s = Session::start(tmp.path(), &path);
    stdout(&koje(tmp.path(), &["new", "kp"]));
    stdout(&s.koje(&["up", "kp", "-s", "empty"]));
    let log = podman_log(tmp.path());
    let data = tmp.path().join("podman.status.voldata");
    assert!(log.contains("volume create koje-kp-home"), "{log}");
    assert!(
        log.contains("unshare sh -c"),
        "the home is shared first: {log}"
    );
    assert!(
        log.contains(&format!("--volume {}:", data.display())) && log.contains(":rslave"),
        "the home volume's directory at $HOME, rslave: {log}"
    );
    assert!(log.contains("--label io.koje.home-mounts=[]"), "{log}");
}

#[test]
fn recreate_asks_and_keeps_the_home() {
    let tmp = tempfile::tempdir().unwrap();
    let path = fake_podman(tmp.path());
    let s = Session::start(tmp.path(), &path);
    stdout(&koje(tmp.path(), &["new", "kp"]));
    // No container yet: nothing to recreate, and no error (koje Settings
    // showed one as a failure).
    let out = stdout(&s.koje(&["recreate", "kp", "--yes"]));
    assert!(
        out.contains("kp has no container yet – its first start creates one"),
        "{out}"
    );
    assert!(!podman_log(tmp.path()).lines().any(|l| l.starts_with("rm ")));

    stdout(&s.koje(&["up", "kp", "-s", "empty"]));
    stdout(&s.koje(&["down", "kp", "--no-save"]));
    // Nobody to answer: nothing happens.
    let out = s.koje(&["recreate", "kp"]);
    assert!(!out.status.success());
    assert!(!podman_log(tmp.path()).lines().any(|l| l == "rm koje-kp"));
    // Stopped: the container goes, the home volume stays, it stays stopped.
    let out = stdout(&s.koje(&["recreate", "kp", "--yes"]));
    assert!(out.contains("removed kp's container"), "{out}");
    let log = podman_log(tmp.path());
    assert!(log.lines().any(|l| l == "rm koje-kp"), "{log}");
    assert!(!log.contains("volume rm"), "{log}");
}

#[test]
fn up_notices_changed_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let path = fake_podman(tmp.path());
    let s = Session::start(tmp.path(), &path);
    stdout(&koje(tmp.path(), &["new", "kp"]));
    stdout(&s.koje(&["up", "kp", "-s", "empty"]));
    let log = podman_log(tmp.path());
    let label = log
        .split_whitespace()
        .find(|w| w.starts_with("io.koje.config="))
        .expect("a fingerprint label on the new container")
        .to_owned();
    stdout(&s.koje(&["down", "kp", "--no-save"]));
    // Same settings: nothing to say.
    std::fs::write(
        tmp.path().join("podman.status.fp"),
        label.trim_start_matches("io.koje.config="),
    )
    .unwrap();
    let out = stdout(&s.koje(&["up", "kp", "-s", "empty"]));
    assert!(!out.contains("changed since"), "{out}");
    stdout(&s.koje(&["down", "kp", "--no-save"]));
    // Another fingerprint (the access profile was changed): a hint, no
    // terminal to ask – the old container starts.
    std::fs::write(tmp.path().join("podman.status.fp"), "v2-0000000000000000").unwrap();
    let out = stdout(&s.koje(&["up", "kp", "-s", "empty"]));
    assert!(out.contains("apply after: koje recreate kp"), "{out}");
    assert!(!podman_log(tmp.path()).lines().any(|l| l == "rm koje-kp"));
}

#[test]
fn stats_and_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let path = fake_podman(tmp.path());
    stdout(&koje(tmp.path(), &["new", "kp"]));
    let out = stdout(&koje_fake(tmp.path(), &path, &["stats"]));
    assert!(out.contains("kp") && out.contains("not created"), "{out}");
    let kp = out.lines().find(|l| l.starts_with("kp ")).unwrap();
    assert_eq!(
        kp.matches('–').count(),
        4,
        "no limits by default (ADR 0035): {kp}"
    );

    let out = stdout(&koje_fake(
        tmp.path(),
        &path,
        &["limit", "kp", "--soft", "6G", "--hard", "8G"],
    ));
    assert!(out.contains("soft 6G, hard 8G"), "{out}");
    let cfg = std::fs::read_to_string(config_of(tmp.path(), "kp")).unwrap();
    assert!(
        cfg.contains("memory_soft = \"6G\"") && cfg.contains("# Host folders"),
        "{cfg}"
    );
    let out = koje_fake(tmp.path(), &path, &["limit", "kp", "--soft", "9G"]);
    assert!(!out.status.success(), "soft above hard");
    // With a default hard limit in config.toml: refused before writing.
    let global = tmp.path().join("config/koje/config.toml");
    let g = std::fs::read_to_string(&global).unwrap();
    std::fs::write(
        &global,
        format!("{g}\n[default_limits]\nmemory_hard = \"4G\"\n"),
    )
    .unwrap();
    let before = std::fs::read_to_string(config_of(tmp.path(), "kp")).unwrap();
    let out = koje_fake(tmp.path(), &path, &["limit", "kp", "--hard", "none"]);
    assert!(
        !out.status.success(),
        "context soft 6G over the default hard 4G"
    );
    assert_eq!(
        std::fs::read_to_string(config_of(tmp.path(), "kp")).unwrap(),
        before,
        "unchanged"
    );
    std::fs::write(&global, g).unwrap();
    let out = stdout(&koje_fake(
        tmp.path(),
        &path,
        &["limit", "kp", "--hard", "none"],
    ));
    assert!(out.contains("soft 6G, hard none"), "{out}");
}
