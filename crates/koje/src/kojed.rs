//! The CLI's side of kojed: the active context for `-c`-less commands.

pub use koje_runtime::kojed::{call, reload};

/// `-c <context>`, else kojed's active context (the focused workspace's, or
/// the focused window's).
pub fn context_or_active(c: &crate::cli::ActiveCtx) -> anyhow::Result<String> {
    if let Some(given) = &c.context {
        return Ok(given.clone());
    }
    let active: String = call("ActiveContext", &())?;
    if active.is_empty() {
        anyhow::bail!(
            "no active context (not on a context's workspace) – name one with -c <context>"
        );
    }
    Ok(active)
}
