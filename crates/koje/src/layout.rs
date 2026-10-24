//! `koje layout …`, `koje stash`, `koje unstash` (M5.5, M5.6): layouts and the
//! stash belong to a context's state and live in kojed's live state while it
//! runs (ADR 0004, 0030); these commands only ask kojed. `koje save`/`down`
//! write them to the state file.

use anyhow::{Result, bail};

use crate::cli::{ActiveCtx, LayoutCmd};
use crate::kojed::call;

/// `-c <context>`, else kojed's active context (the focused workspace's, or
/// the focused window's).
pub(crate) fn ctx(c: &ActiveCtx) -> Result<String> {
    if let Some(given) = &c.context {
        return Ok(given.clone());
    }
    let active: String = call("ActiveContext", &())?;
    if active.is_empty() {
        bail!("no active context (not on a context's workspace) – name one with -c <context>");
    }
    Ok(active)
}

pub fn run(cmd: Option<&LayoutCmd>, name: Option<&str>, active: &ActiveCtx) -> Result<()> {
    match (cmd, name) {
        (None, Some(name)) => switch(&ctx(active)?, name),
        (None, None) => list(&ctx(active)?),
        (Some(LayoutCmd::Next(c)), _) => switch(&ctx(c)?, "+next"),
        (Some(LayoutCmd::Prev(c)), _) => switch(&ctx(c)?, "+prev"),
        (Some(LayoutCmd::New { name, copy, ctx: c }), _) => {
            let c = ctx(c)?;
            call::<_, ()>("NewLayout", &(c.as_str(), name.as_str(), *copy))?;
            println!("new layout {name} in {c}, now active");
            Ok(())
        }
        (Some(LayoutCmd::Save(c)), _) => {
            let c = ctx(c)?;
            call::<_, ()>("SaveLayout", &(c.as_str(),))?;
            println!("saved the current arrangement of {c} into its active layout");
            Ok(())
        }
        (
            Some(LayoutCmd::Autosave {
                name,
                value,
                ctx: c,
            }),
            _,
        ) => {
            let c = ctx(c)?;
            call::<_, ()>(
                "SetLayoutAutosave",
                &(c.as_str(), name.as_str(), value == "on"),
            )?;
            println!("autosave of layout {name} in {c}: {value}");
            Ok(())
        }
        (Some(LayoutCmd::List(c)), _) => list(&ctx(c)?),
        (Some(LayoutCmd::Mv { from, to, ctx: c }), _) => {
            let c = ctx(c)?;
            call::<_, ()>("RenameLayout", &(c.as_str(), from.as_str(), to.as_str()))?;
            println!("renamed layout {from} in {c} to {to}");
            Ok(())
        }
        (Some(LayoutCmd::Rm { name, ctx: c }), _) => {
            let c = ctx(c)?;
            call::<_, ()>("RemoveLayout", &(c.as_str(), name.as_str()))?;
            println!("removed layout {name} from {c}");
            Ok(())
        }
    }
}

fn switch(context: &str, name: &str) -> Result<()> {
    let now: String = call("SwitchLayout", &(context, name))?;
    println!("{context}: layout {now}");
    Ok(())
}

fn list(context: &str) -> Result<()> {
    let (ctx, layouts): (String, Vec<(String, bool, bool, u32)>) = call("Layouts", &(context,))?;
    println!("layouts of {ctx}:");
    for (name, active, autosave, windows) in layouts {
        let mark = if active { "*" } else { " " };
        let auto = if autosave { "" } else { "  (autosave off)" };
        println!("{mark} {name:<16} {windows} window(s){auto}");
    }
    Ok(())
}

pub fn stash(show: bool, active: &ActiveCtx) -> Result<()> {
    if show {
        return call::<_, ()>("ToggleStash", &(ctx(active)?.as_str(),));
    }
    // The focused window decides the context.
    let (ctx, id): (String, String) = call("Stash", &())?;
    println!("stashed {id} of {ctx}");
    Ok(())
}

pub fn unstash(window: Option<&str>, to: Option<&str>, active: &ActiveCtx) -> Result<()> {
    let c = ctx(active)?;
    let id: String = call(
        "Unstash",
        &(c.as_str(), window.unwrap_or(""), to.unwrap_or("")),
    )?;
    println!("unstashed {id} in {c}");
    Ok(())
}
