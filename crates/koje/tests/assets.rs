//! The package's idle and lock files agree on where they are installed.

const IDLE_UNIT: &str = include_str!("../../../assets/systemd/koje-idle.service");
const HYPRIDLE_CONF: &str = include_str!("../../../assets/lock/hypridle.conf");
const CARGO_TOML: &str = include_str!("../Cargo.toml");

/// Regression: hypridle 0.1.7 looks for `hypr/hypridle.conf` in the XDG
/// config dirs despite `--config` and threw without one, so koje-idle never
/// stayed up. The unit's config has to lie in `<XDG_CONFIG_DIRS entry>/hypr/`.
#[test]
fn idle_config_is_found_through_xdg_config_dirs() {
    let exec = IDLE_UNIT
        .lines()
        .find_map(|l| l.strip_prefix("ExecStart="))
        .expect("ExecStart");
    let dir = exec
        .split_whitespace()
        .find_map(|w| w.strip_prefix("XDG_CONFIG_DIRS="))
        .and_then(|dirs| dirs.split(':').next())
        .expect("XDG_CONFIG_DIRS set first");
    let config = format!("{dir}/hypr/hypridle.conf");
    assert!(exec.ends_with(&format!("--config {config}")), "{exec}");
    assert!(
        HYPRIDLE_CONF.contains(&format!("hyprlock --config {dir}/hypr/hyprlock.conf")),
        "lock_cmd"
    );
    let installed = format!("\"{}/hypr/\"", dir.trim_start_matches('/'));
    for file in ["hypridle.conf", "hyprlock.conf"] {
        assert!(
            CARGO_TOML.contains(&format!(
                "[\"../../assets/lock/{file}\", {installed}, \"644\"]"
            )),
            "{file} installed to {installed}"
        );
    }
}
