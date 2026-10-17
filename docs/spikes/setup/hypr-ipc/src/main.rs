//! M2.2 spike: Hyprland IPC from Rust, std only (plus serde_json for output).
//!
//!   hypr-ipc clients                       list windows (j/clients)
//!   hypr-ipc events                        print socket2 events (Ctrl+C to stop)
//!   hypr-ipc move <addr> <x> <y> <w> <h>   move + resize one window in one batch
//!   hypr-ipc color <addr> <rrggbb>         per-window border color (tries known syntaxes)
//!   hypr-ipc tagcolor <addr> <tag> <rrggbb> rule per tag + tag the window
//!   hypr-ipc bench [n]                     move n windows: one batch vs. n×2 calls
//!   hypr-ipc raw <request>                 send a raw request, print the reply
//!
//! Instance: $HYPRLAND_INSTANCE_SIGNATURE, or the newest one in
//! $XDG_RUNTIME_DIR/hypr/ (a nested Hyprland started from GNOME has no
//! variable in the GNOME terminal). Override: HYPR_INSTANCE=<signature>.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Instant;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn instance_dir() -> Result<PathBuf> {
    let run = std::env::var("XDG_RUNTIME_DIR").map_err(|_| "XDG_RUNTIME_DIR is not set (run this on the host)")?;
    let base = PathBuf::from(run).join("hypr");
    if let Ok(sig) = std::env::var("HYPR_INSTANCE").or_else(|_| std::env::var("HYPRLAND_INSTANCE_SIGNATURE")) {
        return Ok(base.join(sig));
    }
    let newest = std::fs::read_dir(&base)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join(".socket.sock").exists())
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .ok_or("no Hyprland instance found in $XDG_RUNTIME_DIR/hypr")?;
    Ok(newest.path())
}

/// One request on the command socket; Hyprland closes the connection after replying.
fn request(cmd: &str) -> Result<String> {
    let mut s = UnixStream::connect(instance_dir()?.join(".socket.sock"))?;
    s.write_all(cmd.as_bytes())?;
    let mut reply = String::new();
    s.read_to_string(&mut reply)?;
    Ok(reply)
}

fn clients() -> Result<Vec<serde_json::Value>> {
    let reply = request("j/clients")?;
    Ok(serde_json::from_str::<serde_json::Value>(&reply)?
        .as_array()
        .cloned()
        .unwrap_or_default())
}

fn move_cmds(addr: &str, x: i64, y: i64, w: i64, h: i64) -> [String; 2] {
    [
        format!("dispatch movewindowpixel exact {x} {y},address:{addr}"),
        format!("dispatch resizewindowpixel exact {w} {h},address:{addr}"),
    ]
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| -> Result<&str> { args.get(i).map(String::as_str).ok_or_else(|| "missing argument (see source header)".into()) };
    let num = |i: usize| -> Result<i64> { Ok(arg(i)?.parse()?) };
    eprintln!("instance: {}", instance_dir()?.display());

    match arg(0)? {
        "clients" => {
            println!("version: {}", request("j/version")?.lines().find(|l| l.contains("\"tag\"")).unwrap_or("?").trim());
            for c in clients()? {
                let pair = |v: &serde_json::Value| format!("{},{}", v[0], v[1]);
                println!(
                    "{}  pid {:>7}  ws {:<12} at {:<10} size {:<10} {} – {}",
                    c["address"].as_str().unwrap_or("?"),
                    c["pid"],
                    c["workspace"]["name"].as_str().unwrap_or("?"),
                    pair(&c["at"]), pair(&c["size"]),
                    c["class"].as_str().unwrap_or(""),
                    c["title"].as_str().unwrap_or(""),
                );
            }
        }
        "events" => {
            let s = UnixStream::connect(instance_dir()?.join(".socket2.sock"))?;
            for line in BufReader::new(s).lines() {
                println!("{}", line?);
            }
        }
        "move" => {
            let [a, b] = move_cmds(arg(1)?, num(2)?, num(3)?, num(4)?, num(5)?);
            println!("{}", request(&format!("[[BATCH]]{a};{b}"))?);
        }
        "color" => {
            let (addr, rgb) = (arg(1)?, arg(2)?);
            // Property names changed between Hyprland releases (0.53: snake_case
            // rule properties); record which request works.
            for cmd in [
                format!("dispatch setprop address:{addr} border_color rgb({rgb})"),
                format!("dispatch setprop address:{addr} active_border_color rgb({rgb})"),
                format!("dispatch setprop address:{addr} activebordercolor rgb({rgb})"),
                format!("dispatch setprop address:{addr} bordercolor rgb({rgb})"),
            ] {
                println!("{cmd}\n  -> {}", request(&cmd)?.trim());
            }
        }
        "tagcolor" => {
            // koje-style: one rule per context matching a tag, then tag the window.
            let (addr, tag, rgb) = (arg(1)?, arg(2)?, arg(3)?);
            for cmd in [
                format!("keyword windowrule match:tag {tag}, border_color rgb({rgb})"),
                format!("dispatch tagwindow +{tag} address:{addr}"),
            ] {
                println!("{cmd}\n  -> {}", request(&cmd)?.trim());
            }
        }
        "bench" => {
            let n: usize = args.get(1).map_or(Ok(5), |s| s.parse())?;
            let cs: Vec<String> = clients()?.iter().filter_map(|c| c["address"].as_str().map(str::to_owned)).take(n).collect();
            if cs.is_empty() {
                return Err("no windows to move".into());
            }
            // Two arrangements, so every run really moves the windows.
            for round in 0..4 {
                let cmds: Vec<String> = cs.iter().enumerate().flat_map(|(i, a)| {
                    let i = i64::try_from(i).unwrap_or(0);
                    let off = if round % 2 == 0 { 0 } else { 60 };
                    move_cmds(a, 40 + i * 120 + off, 40 + i * 60, 640, 400)
                }).collect();
                let t = Instant::now();
                if round < 2 {
                    request(&format!("[[BATCH]]{}", cmds.join(";")))?;
                    println!("batch    {} windows ({} commands): {:?}", cs.len(), cmds.len(), t.elapsed());
                } else {
                    for c in &cmds {
                        request(c)?;
                    }
                    println!("separate {} windows ({} requests): {:?}", cs.len(), cmds.len(), t.elapsed());
                }
            }
        }
        "raw" => println!("{}", request(&args[1..].join(" "))?),
        other => return Err(format!("unknown command {other:?} (see source header)").into()),
    }
    Ok(())
}
