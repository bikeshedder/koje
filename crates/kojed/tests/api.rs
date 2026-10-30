//! kojed's D-Bus API on a private `dbus-daemon`, without Hyprland (kojed waits
//! for it and serves clients meanwhile). Skipped when `dbus-daemon` is missing.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use koje_core::api::{BUS_NAME, INTERFACE, OBJECT_PATH};
use koje_core::color::Color;
use koje_core::name::ContextName;
use koje_core::paths::Paths;
use zbus::zvariant::{OwnedValue, Value};

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

    serves_the_interface_file(&conn).await;

    let list: Vec<Row> = call(&conn, "Contexts", &()).await;
    assert_eq!(list, [row("work", "#FF0000", "stopped")]);
    let active: String = call(&conn, "ActiveContext", &()).await;
    assert_eq!(active, "");
    // Focus needs GNOME Shell; the error says so instead of hanging.
    let err = conn
        .call_method(
            Some(BUS_NAME),
            OBJECT_PATH,
            Some(INTERFACE),
            "Focus",
            &("work",),
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("compositor"), "{err}");
    let err = conn
        .call_method(
            Some(BUS_NAME),
            OBJECT_PATH,
            Some(INTERFACE),
            "Focus",
            &("Bad Name",),
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("InvalidArgs"), "{err}");

    // `koje new` writes the file, then asks kojed to reload.
    create(root, "home", Color::rgb(0, 0, 0xff));
    let (): () = call(&conn, "Reload", &()).await;
    let list: Vec<Row> = call(&conn, "Contexts", &()).await;
    assert_eq!(
        list,
        [
            row("home", "#0000FF", "stopped"),
            row("work", "#FF0000", "stopped")
        ]
    );

    settings_app_calls(&conn).await;
}

/// The contract (ADR 0060): what kojed serves is the interface file.
async fn serves_the_interface_file(conn: &zbus::Connection) {
    let served = zbus::fdo::IntrospectableProxy::builder(conn)
        .destination(BUS_NAME)
        .unwrap()
        .path(OBJECT_PATH)
        .unwrap()
        .build()
        .await
        .unwrap()
        .introspect()
        .await
        .unwrap();
    let file = include_str!("../dbus/io.koje.Daemon1.xml");
    assert_eq!(
        members(&served),
        members(file),
        "crates/kojed/dbus/io.koje.Daemon1.xml differs from what kojed serves:\n{served}"
    );
}

/// The settings app's calls (G.23).
async fn settings_app_calls(conn: &zbus::Connection) {
    let conn = conn.clone();
    let name: String = call(&conn, "NewContext", &("shop",)).await;
    assert_eq!(name, "shop");
    let err = call_err(&conn, "NewContext", &("shop",)).await;
    assert!(err.contains("io.koje.Error.InvalidConfig"), "{err}");
    let config: HashMap<String, OwnedValue> = call(&conn, "Config", &("shop",)).await;
    assert_eq!(
        String::try_from(config["name"].try_clone().unwrap()).unwrap(),
        "shop"
    );
    let change: HashMap<&str, Value<'_>> = [("label", Value::from("Shop"))].into();
    let notes: Vec<String> = call(&conn, "SetConfig", &("shop", change)).await;
    assert_eq!(notes, Vec::<String>::new());
    let list: Vec<Row> = call(&conn, "Contexts", &()).await;
    assert_eq!(list[1].1, "Shop");
    let bad: HashMap<&str, Value<'_>> = [("color", Value::from("green"))].into();
    let err = call_err(&conn, "SetConfig", &("shop", bad)).await;
    assert!(err.contains("io.koje.Error.InvalidConfig"), "{err}");
    let err = call_err(&conn, "Config", &("nope",)).await;
    assert!(err.contains("io.koje.Error.NotFound"), "{err}");

    let (): () = call(&conn, "CopyState", &("shop", "empty", "default")).await;
    let states: Vec<StateRow> = call(&conn, "States", &("shop",)).await;
    let default = states.iter().find(|s| s.0 == "default").unwrap();
    assert_eq!((default.1.as_str(), default.3), ("default", true));
    let (): () = call(&conn, "RemoveState", &("shop", "default")).await;

    // The extension's dash (G.32).
    let ids = vec!["foot.desktop".to_owned()];
    let (): () = call(&conn, "SetFavorites", &("shop", true, &ids)).await;
    let got: (bool, Vec<String>) = call(&conn, "Favorites", &("shop",)).await;
    assert_eq!(got, (true, ids));
    let err = call_err(&conn, "SetFavorites", &("shop", true, vec!["foot"])).await;
    assert!(err.contains("io.koje.Error.InvalidConfig"), "{err}");
    let err = call_err(&conn, "Up", &("shop", "", "later", true, true)).await;
    assert!(err.contains("io.koje.Error.InvalidArgs"), "{err}");
}

/// `(name, role, current, stored, windows, layouts, modified, error)` of `States`.
type StateRow = (String, String, bool, bool, u32, u32, i64, String);

/// `(name, label, icon, color, status, windows)` of `Contexts`.
type Row = (String, String, String, String, String, u32);

fn row(name: &str, color: &str, status: &str) -> Row {
    (
        name.to_owned(),
        name.to_owned(),
        koje_core::look::CONTEXT_ICON.to_owned(),
        color.to_owned(),
        status.to_owned(),
        0,
    )
}

async fn call_err<B>(conn: &zbus::Connection, method: &str, body: &B) -> String
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    conn.call_method(Some(BUS_NAME), OBJECT_PATH, Some(INTERFACE), method, body)
        .await
        .unwrap_err()
        .to_string()
}

/// `method Name(in)(out)` / `signal Name(args)()` of every member of
/// `io.koje.Daemon1` in introspection XML.
fn members(xml: &str) -> BTreeSet<String> {
    use quick_xml::events::{BytesStart, Event};
    fn attr(e: &BytesStart<'_>, name: &str) -> String {
        e.attributes()
            .flatten()
            .find(|a| a.key.as_ref() == name)
            .map(|a| {
                a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                    .unwrap()
                    .into_owned()
            })
            .unwrap_or_default()
    }
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut out = BTreeSet::new();
    let mut ours = false;
    let mut member: Option<(String, String, String)> = None;
    loop {
        let (e, empty) = match reader.read_event().unwrap() {
            Event::Start(e) => (e, false),
            Event::Empty(e) => (e, true),
            Event::End(e) => {
                if matches!(e.name().as_ref(), "method" | "signal")
                    && let Some((name, ins, outs)) = member.take()
                {
                    out.insert(format!("{name}({ins})({outs})"));
                }
                continue;
            }
            Event::Eof => return out,
            _ => continue,
        };
        match e.name().as_ref() {
            "interface" => ours = attr(&e, "name") == INTERFACE,
            kind @ ("method" | "signal") if ours => {
                let name = format!("{kind} {}", attr(&e, "name"));
                if empty {
                    out.insert(format!("{name}()()"));
                } else {
                    member = Some((name, String::new(), String::new()));
                }
            }
            "arg" => {
                if let Some((_, ins, outs)) = &mut member {
                    let into = if attr(&e, "direction") == "out" {
                        outs
                    } else {
                        ins
                    };
                    into.push_str(&attr(&e, "type"));
                }
            }
            _ => {}
        }
    }
}
