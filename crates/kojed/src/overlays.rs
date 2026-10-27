//! Overlays' key bindings (M7.1): what `config.toml` configures
//! (`koje_core::overlay`) bound in Hyprland at start, when the file changes
//! and after Hyprland reloaded its config or restarted (both drop bindings
//! made with `keyword`). An invalid file keeps the bindings there are.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use koje_core::config::GlobalConfig;
use koje_core::overlay::{self, Bind};
use koje_hyprland::{Hyprland, requests};
use tokio::sync::Notify;

/// How often `config.toml` is looked at: kojed has no file watcher (ADR 0026).
const POLL: Duration = Duration::from_secs(2);
const TIMEOUT: Duration = Duration::from_secs(5);

pub async fn run(hypr: Hyprland, config: PathBuf, reapply: Arc<Notify>) {
    let mut applied: Vec<Bind> = Vec::new();
    let mut seen: Option<Option<SystemTime>> = None;
    let mut wanted: Vec<Bind> = Vec::new();
    loop {
        let modified = std::fs::metadata(&config).and_then(|m| m.modified()).ok();
        if seen != Some(modified) {
            seen = Some(modified);
            match GlobalConfig::load(&config).and_then(|c| overlay::overlays(&c)) {
                Ok(o) => wanted = overlay::binds(&o),
                Err(e) => tracing::warn!("overlays: {e} – keeping the bindings there are"),
            }
        }
        let send = overlay::changes(&applied, &wanted);
        if !send.is_empty() {
            match apply(&hypr, &send).await {
                Ok(()) => {
                    tracing::info!(count = send.len(), "overlay bindings applied");
                    applied.clone_from(&wanted);
                }
                // Hyprland not up yet: `reapply` follows when it is.
                Err(e) => tracing::warn!("binding the overlays' keys: {e:#}"),
            }
        }
        tokio::select! {
            () = tokio::time::sleep(POLL) => {}
            () = reapply.notified() => applied.clear(),
        }
    }
}

async fn apply(hypr: &Hyprland, lines: &[String]) -> anyhow::Result<()> {
    let request = requests::batch(lines);
    let reply = tokio::time::timeout(TIMEOUT, hypr.request(&request)).await??;
    requests::check_reply(&request, &reply)?;
    Ok(())
}
