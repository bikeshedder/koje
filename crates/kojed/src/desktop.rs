//! `[desktop]` from `config.toml` (ADR 0056): the keys that are set applied
//! to Hyprland with `keyword` at start, when the file changes and after
//! Hyprland reloaded its config or restarted (both reset `keyword`
//! settings). A key that is no longer set goes back to koje's default. An
//! invalid file keeps what is applied. Decisions: `koje_core::desktop`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use koje_core::config::GlobalConfig;
use koje_core::desktop;
use koje_hyprland::{Hyprland, requests};
use tokio::sync::Notify;

/// How often `config.toml` is looked at: kojed has no file watcher (ADR 0026).
const POLL: Duration = Duration::from_secs(2);
const TIMEOUT: Duration = Duration::from_secs(5);

pub async fn run(hypr: Hyprland, config: PathBuf, reapply: Arc<Notify>) {
    let mut applied: BTreeMap<&'static str, String> = BTreeMap::new();
    let mut seen: Option<Option<SystemTime>> = None;
    let mut wanted: BTreeMap<&'static str, String> = BTreeMap::new();
    loop {
        let modified = std::fs::metadata(&config).and_then(|m| m.modified()).ok();
        if seen != Some(modified) {
            seen = Some(modified);
            match GlobalConfig::load(&config) {
                Ok(c) => wanted = desktop::wanted(&c.desktop),
                Err(e) => tracing::warn!("[desktop]: {e} – keeping what is applied"),
            }
        }
        let send = desktop::changes(&applied, &wanted);
        // koje's plugin may not be loaded: its keys must not hold up the rest.
        let (plugin, hyprland): (Vec<String>, Vec<String>) = send
            .into_iter()
            .partition(|l| l.starts_with("keyword plugin:"));
        let mut done = true;
        if !hyprland.is_empty() {
            match apply(&hypr, &hyprland).await {
                Ok(()) => tracing::info!(count = hyprland.len(), "[desktop] applied"),
                // Hyprland not up yet: `reapply` follows when it is.
                Err(e) => {
                    tracing::warn!("applying [desktop]: {e:#}");
                    done = false;
                }
            }
        }
        if done
            && !plugin.is_empty()
            && let Err(e) = apply(&hypr, &plugin).await
        {
            tracing::info!("[desktop] for koje's plugin not applied (is it loaded?): {e:#}");
        }
        if done {
            applied.clone_from(&wanted);
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
