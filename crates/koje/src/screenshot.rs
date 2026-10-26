//! `koje screenshot area|screen|window`: as GNOME takes them – the file in
//! the Pictures folder's `Screenshots`, the image on the clipboard, a
//! notification. With grim, slurp (an area) and wl-copy; Hyprland names the
//! screen and the window. Decisions: `koje_core::screenshot`.

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context as _, Result, bail};
use koje_core::screenshot::{self, Kind};
use serde::Deserialize;

pub fn run(kind: Kind) -> Result<()> {
    let Some(target) = target(kind)? else {
        return Ok(()); // the area was cancelled (Escape)
    };
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?);
    let config =
        std::env::var_os("XDG_CONFIG_HOME").map_or_else(|| home.join(".config"), PathBuf::from);
    let user_dirs = std::fs::read_to_string(config.join("user-dirs.dirs")).ok();
    let dir = screenshot::pictures_dir(user_dirs.as_deref(), &home).join("Screenshots");
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let stamp = Command::new("date")
        .arg("+%Y-%m-%d %H-%M-%S")
        .output()
        .context("running date")?;
    let file = dir.join(screenshot::file_name(&String::from_utf8_lossy(
        &stamp.stdout,
    )));

    let mut grim = Command::new("grim");
    match &target {
        Target::Geometry(g) => grim.args(["-g", g]),
        Target::Output(o) => grim.args(["-o", o]),
    };
    let status = grim.arg(&file).status().context("running grim")?;
    if !status.success() {
        bail!("grim failed: {status}");
    }
    let copied = std::fs::File::open(&file).and_then(|png| {
        Command::new("wl-copy")
            .args(["--type", "image/png"])
            .stdin(png)
            .status()
    });
    if !copied.is_ok_and(|s| s.success()) {
        eprintln!("warning: the screenshot is not on the clipboard (wl-copy)");
    }
    if let Err(e) = notify(&file) {
        eprintln!("warning: no notification: {e:#}");
    }
    println!("{}", file.display());
    Ok(())
}

enum Target {
    /// `x,y wxh`
    Geometry(String),
    /// A screen by its connector.
    Output(String),
}

#[derive(Deserialize)]
struct Monitor {
    name: String,
    #[serde(default)]
    focused: bool,
}

#[derive(Deserialize)]
struct ActiveWindow {
    at: Option<[i32; 2]>,
    size: Option<[i32; 2]>,
}

fn hyprland(request: &str) -> Result<String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting the Hyprland client")?;
    Ok(runtime.block_on(koje_hyprland::Hyprland::from_env().request(request))?)
}

/// What to give grim; `None` when the user cancelled choosing an area.
fn target(kind: Kind) -> Result<Option<Target>> {
    Ok(Some(match kind {
        Kind::Area => {
            let out = Command::new("slurp").output().context("running slurp")?;
            if !out.status.success() {
                return Ok(None);
            }
            Target::Geometry(String::from_utf8_lossy(&out.stdout).trim().to_owned())
        }
        Kind::Screen => {
            let monitors: Vec<Monitor> =
                serde_json::from_str(&hyprland("j/monitors")?).context("Hyprland's monitors")?;
            let m = monitors
                .into_iter()
                .find(|m| m.focused)
                .context("no screen has the focus")?;
            Target::Output(m.name)
        }
        Kind::Window => {
            let w: ActiveWindow = serde_json::from_str(&hyprland("j/activewindow")?)
                .context("Hyprland's active window")?;
            let (Some([x, y]), Some([width, height])) = (w.at, w.size) else {
                bail!("no window has the focus");
            };
            Target::Geometry(screenshot::geometry(x, y, width, height))
        }
    }))
}

/// GNOME Shell's notification, with the picture.
fn notify(file: &std::path::Path) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        use zbus::zvariant::Value;
        let conn = zbus::Connection::session().await?;
        let path = file.display().to_string();
        let hints: std::collections::HashMap<&str, Value<'_>> =
            [("image-path", Value::from(path.as_str()))]
                .into_iter()
                .collect();
        conn.call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                "Screenshot",
                0u32,
                "screenshot-recorded-symbolic",
                screenshot::SUMMARY,
                screenshot::BODY,
                Vec::<&str>::new(),
                hints,
                -1i32,
            ),
        )
        .await?;
        anyhow::Ok(())
    })
}
