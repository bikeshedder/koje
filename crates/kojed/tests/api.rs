//! kojed's D-Bus API on a private `dbus-daemon`, without Hyprland (kojed waits
//! for it and serves clients meanwhile). Skipped when `dbus-daemon` is missing.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use koje_core::api::{BUS_NAME, INTERFACE, OBJECT_PATH};
use koje_core::color::Color;
use koje_core::name::ContextName;
use koje_core::paths::Paths;

/// A private `dbus-daemon`, killed on drop.
struct Bus {
    address: String,
    pid: u32,
}

impl Bus {
    fn start() -> Option<Self> {
        let out = Command::new("dbus-daemon")
            .args(["--session", "--fork", "--print-address=1", "--print-pid=2"])
            .output()
            .ok()?;
        let address = String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()?
            .to_owned();
        let pid = String::from_utf8_lossy(&out.stderr).trim().parse().ok()?;
        Some(Self { address, pid })
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = Command::new("kill").arg(self.pid.to_string()).status();
    }
}

struct Kojed(Child);

impl Drop for Kojed {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn paths(root: &Path) -> Paths {
    Paths::with_roots(
        root.join("config/koje"),
        root.join("data/koje"),
        root.join("state/koje"),
    )
}

fn create(root: &Path, name: &str, color: Color) {
    koje_core::context::create(
        &paths(root),
        &ContextName::new(name).unwrap(),
        None,
        Some(color),
    )
    .unwrap();
}

async fn call<B, R>(conn: &zbus::Connection, method: &str, body: &B) -> R
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
    R: for<'d> serde::Deserialize<'d> + zbus::zvariant::Type,
{
    conn.call_method(Some(BUS_NAME), OBJECT_PATH, Some(INTERFACE), method, body)
        .await
        .unwrap()
        .body()
        .deserialize()
        .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn serves_contexts_without_hyprland() {
    let Some(bus) = Bus::start() else {
        eprintln!("dbus-daemon not installed: skipping");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("run")).unwrap();
    create(root, "work", Color::rgb(0xff, 0, 0));
    let _kojed = Kojed(
        Command::new(env!("CARGO_BIN_EXE_kojed"))
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", root)
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("XDG_STATE_HOME", root.join("state"))
            .env("XDG_RUNTIME_DIR", root.join("run"))
            .env("DBUS_SESSION_BUS_ADDRESS", &bus.address)
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let conn = zbus::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .build()
        .await
        .unwrap();
    let dbus = zbus::fdo::DBusProxy::new(&conn).await.unwrap();
    let mut up = false;
    for _ in 0..100 {
        if dbus
            .name_has_owner(BUS_NAME.try_into().unwrap())
            .await
            .unwrap()
        {
            up = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(up, "kojed did not take {BUS_NAME}");

    let list: Vec<(String, String, u32)> = call(&conn, "ListContexts", &()).await;
    assert_eq!(list, [("work".to_owned(), "#FF0000".to_owned(), 0)]);
    let active: String = call(&conn, "ActiveContext", &()).await;
    assert_eq!(active, "");

    // `koje new` writes the file, then asks kojed to reload.
    create(root, "home", Color::rgb(0, 0, 0xff));
    let (): () = call(&conn, "Reload", &()).await;
    let list: Vec<(String, String, u32)> = call(&conn, "ListContexts", &()).await;
    assert_eq!(
        list,
        [
            ("home".to_owned(), "#0000FF".to_owned(), 0),
            ("work".to_owned(), "#FF0000".to_owned(), 0)
        ]
    );
}
