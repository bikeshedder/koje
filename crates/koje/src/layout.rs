//! `koje layout …`, `koje stash`, `koje unstash` (M5.5, M5.6): layouts and the
//! stash live in kojed's live state of a running context (ADR 0030); these
//! commands only ask kojed. `koje save`/`down` write them to the state file.

use anyhow::Result;

use crate::cli::{ActiveCtx, LayoutCmd};
use crate::kojed::call;

/// `-c <context>`, or `""` for kojed's active context.
fn ctx(c: &ActiveCtx) -> &str {
    c.context.as_deref().unwrap_or("")
}

pub fn run(cmd: Option<&LayoutCmd>, name: Option<&str>, active: &ActiveCtx) -> Result<()> {
    match (cmd, name) {
        (None, Some(name)) => switch(ctx(active), name),
        (None, None) => list(ctx(active)),
        (Some(LayoutCmd::Next(c)), _) => switch(ctx(c), "+next"),
        (Some(LayoutCmd::Prev(c)), _) => switch(ctx(c), "+prev"),
        (Some(LayoutCmd::New { name, copy, ctx: c }), _) => {
            call::<_, ()>("NewLayout", &(ctx(c), name.as_str(), *copy))?;
            println!("new layout {name}, now active");
            Ok(())
        }
        (Some(LayoutCmd::Save(c)), _) => {
            call::<_, ()>("SaveLayout", &(ctx(c),))?;
            println!("saved the current arrangement into the active layout");
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
            call::<_, ()>("SetLayoutAutosave", &(ctx(c), name.as_str(), value == "on"))?;
            println!("autosave of {name}: {value}");
            Ok(())
        }
        (Some(LayoutCmd::List(c)), _) => list(ctx(c)),
        (Some(LayoutCmd::Mv { from, to, ctx: c }), _) => {
            call::<_, ()>("RenameLayout", &(ctx(c), from.as_str(), to.as_str()))?;
            println!("renamed layout {from} to {to}");
            Ok(())
        }
        (Some(LayoutCmd::Rm { name, ctx: c }), _) => {
            call::<_, ()>("RemoveLayout", &(ctx(c), name.as_str()))?;
            println!("removed layout {name}");
            Ok(())
        }
    }
}

fn switch(context: &str, name: &str) -> Result<()> {
    let now: String = call("SwitchLayout", &(context, name))?;
    println!("layout {now}");
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
        return call::<_, ()>("ToggleStash", &(ctx(active),));
    }
    let (ctx, id): (String, String) = call("Stash", &())?;
    println!("stashed {id} of {ctx}");
    Ok(())
}

pub fn unstash(window: Option<&str>, to: Option<&str>, active: &ActiveCtx) -> Result<()> {
    let id: String = call(
        "Unstash",
        &(ctx(active), window.unwrap_or(""), to.unwrap_or("")),
    )?;
    println!("unstashed {id}");
    Ok(())
}
