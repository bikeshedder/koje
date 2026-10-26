//! The gsettings bridge (ADR 0044, M8.43): GNOME's input settings – keyboard
//! layouts and options, key repeat, mouse, touchpad – applied to Hyprland at
//! start, on every change GNOME Settings makes, and after Hyprland reloaded
//! its config or restarted (both reset `keyword` settings). Read with
//! `gsettings list-recursively`, watched with `gsettings monitor`: the same
//! values, defaults included, GNOME Settings writes. Decisions:
//! `koje_core::gsettings`.

use std::sync::Arc;
use std::time::Duration;

use koje_core::gsettings::{self, INPUT_SCHEMAS, Settings};
use koje_hyprland::{Hyprland, requests};
use tokio::io::{AsyncBufReadExt as _, BufReader};
use tokio::sync::{Notify, mpsc};

/// Changes come in bursts (GNOME Settings writes several keys): apply once.
const SETTLE: Duration = Duration::from_millis(100);
/// A `gsettings monitor` that ended (schema missing, gsettings gone) is
/// started again after this.
const RESTART: Duration = Duration::from_secs(60);
const TIMEOUT: Duration = Duration::from_secs(5);

/// Follow GNOME's input settings and apply them to Hyprland. `reapply`: kojed
/// saw Hyprland reload its config or start (again).
pub async fn run(hypr: Hyprland, reapply: Arc<Notify>) {
    if !on_path("gsettings") {
        tracing::info!("no gsettings: GNOME's input settings are not applied (ADR 0044)");
        return;
    }
    let (tx, mut changes) = mpsc::unbounded_channel();
    for schema in INPUT_SCHEMAS {
        tokio::spawn(watch(schema, tx.clone()));
    }
    let mut applied: Vec<(String, String)> = Vec::new();
    loop {
        let now = gsettings::input_keywords(&read().await);
        let send: Vec<&(String, String)> = gsettings::changed(&now, &applied);
        if !send.is_empty() {
            match apply(&hypr, &send).await {
                Ok(()) => {
                    tracing::info!(count = send.len(), "applied GNOME's input settings");
                    applied = now;
                }
                // Hyprland not up yet: `reapply` follows when it is.
                Err(e) => tracing::warn!("applying GNOME's input settings: {e:#}"),
            }
        }
        tokio::select! {
            Some(()) = changes.recv() => {
                tokio::time::sleep(SETTLE).await;
                while changes.try_recv().is_ok() {}
            }
            () = reapply.notified() => applied.clear(),
        }
    }
}

fn on_path(command: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(command).is_file()))
}

/// Every input schema's keys and values; a schema that is not installed is
/// left out.
async fn read() -> Settings {
    let mut settings = Settings::new();
    for schema in INPUT_SCHEMAS {
        let out = tokio::time::timeout(
            TIMEOUT,
            tokio::process::Command::new("gsettings")
                .args(["list-recursively", schema])
                .kill_on_drop(true)
                .output(),
        )
        .await;
        match out {
            Ok(Ok(out)) if out.status.success() => {
                settings.insert(
                    schema.to_owned(),
                    gsettings::parse_list(&String::from_utf8_lossy(&out.stdout)),
                );
            }
            Ok(Ok(out)) => tracing::debug!(
                schema,
                "gsettings: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
            Ok(Err(e)) => tracing::debug!(schema, "gsettings: {e}"),
            Err(_) => tracing::warn!(schema, "gsettings did not answer"),
        }
    }
    settings
}

/// `gsettings monitor <schema>`: a line per change; each one is a nudge to
/// read everything again.
async fn watch(schema: &'static str, tx: mpsc::UnboundedSender<()>) {
    loop {
        let child = tokio::process::Command::new("gsettings")
            .args(["monitor", schema])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn();
        match child {
            Ok(mut child) => {
                if let Some(out) = child.stdout.take() {
                    let mut lines = BufReader::new(out).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        tracing::debug!(schema, %line, "GNOME setting changed");
                        if tx.send(()).is_err() {
                            return;
                        }
                    }
                }
                let _ = child.wait().await;
            }
            Err(e) => tracing::debug!(schema, "gsettings monitor: {e}"),
        }
        tokio::time::sleep(RESTART).await;
    }
}

/// Hyprland's `keyword` for each setting, in one request.
async fn apply(hypr: &Hyprland, settings: &[&(String, String)]) -> anyhow::Result<()> {
    let lines: Vec<String> = settings
        .iter()
        .map(|(name, value)| keyword(name, value))
        .collect();
    let request = requests::batch(&lines);
    let reply = tokio::time::timeout(TIMEOUT, hypr.request(&request)).await??;
    requests::check_reply(&request, &reply)?;
    Ok(())
}

/// `keyword name value`; Hyprland's spelling of an empty value is
/// `[[EMPTY]]`.
fn keyword(name: &str, value: &str) -> String {
    let value = if value.is_empty() { "[[EMPTY]]" } else { value };
    format!("keyword {name} {value}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keywords_with_empty_values() {
        assert_eq!(
            keyword("input:kb_layout", "de,us"),
            "keyword input:kb_layout de,us"
        );
        assert_eq!(
            keyword("input:kb_options", ""),
            "keyword input:kb_options [[EMPTY]]"
        );
    }
}
