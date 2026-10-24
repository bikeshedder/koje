//! `koje terminal` (ADR 0037): Super+Return opens a terminal where the user
//! looks – in the active context, on the host outside one.

use anyhow::{Context as _, Result};
use koje_core::paths::Paths;

use crate::cli::ActiveCtx;

/// The terminal in both places: the base image ships it (ADR 0023), `koje
/// doctor` checks the host's.
pub const TERMINAL: &str = "foot";

#[derive(Debug, PartialEq, Eq)]
enum Place {
    Context(String),
    Host,
}

/// Where the active context is: kojed's answer. No active context, or no
/// kojed to ask, means the host – the binding always opens a terminal.
fn place(active: Result<String>) -> Place {
    match active {
        Ok(active) if !active.is_empty() => Place::Context(active),
        Ok(_) => Place::Host,
        Err(e) => {
            tracing::warn!("no active context, terminal on the host: {e:#}");
            Place::Host
        }
    }
}

/// `koje terminal [-c <context>]`.
pub fn run(paths: &Paths, ctx: &ActiveCtx) -> Result<()> {
    let place = match &ctx.context {
        Some(given) => Place::Context(given.clone()),
        None => place(crate::kojed::call("ActiveContext", &())),
    };
    match place {
        // The active context's workspace is in view already; a named one is
        // switched to, like `koje run`.
        Place::Context(name) => {
            crate::runtime::run(paths, &name, &[TERMINAL.to_owned()], ctx.context.is_some())
        }
        Place::Host => {
            use std::os::unix::process::CommandExt as _;
            std::process::Command::new(TERMINAL)
                .current_dir(std::env::var_os("HOME").unwrap_or_else(|| "/".into()))
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                // Its own process group: it outlives koje and its signals.
                .process_group(0)
                .spawn()
                .with_context(|| format!("starting {TERMINAL} on the host"))?;
            println!("started {TERMINAL} on the host");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_context_else_host() {
        assert_eq!(place(Ok("kp".into())), Place::Context("kp".into()));
        assert_eq!(place(Ok(String::new())), Place::Host);
        assert_eq!(
            place(Err(anyhow::anyhow!("kojed is not running"))),
            Place::Host
        );
    }
}
