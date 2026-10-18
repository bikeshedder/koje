//! Command line definition. Mirrors the command table in the concept
//! (`docs/concept/koje-konzept.pdf`, section "Das Werkzeug: koje") and
//! `docs/decisions/0005-named-states-and-cli.md`. Change both together.

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "koje", version, about = "Isolated work contexts on Hyprland", long_about = None)]
pub struct Cli {
    /// More output (-v info, -vv debug, -vvv trace). Also respects `RUST_LOG`.
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Command,
}

/// Context name argument.
#[derive(Debug, Args)]
pub struct Ctx {
    /// Context name, e.g. `kundenportal`.
    pub context: String,
}

/// Optional context; defaults to the context of the focused window.
#[derive(Debug, Args)]
pub struct ActiveCtx {
    /// Context to act on (default: the context of the focused window).
    #[arg(short, long)]
    pub context: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start a context and restore a state; if running, switch to the state.
    Up {
        #[command(flatten)]
        ctx: Ctx,
        /// State to load (default: the `default` role).
        #[arg(short, long)]
        state: Option<String>,
        /// When switching a running context: do not save the current windows
        /// into the `default` role first.
        #[arg(long)]
        no_save: bool,
        /// After a crash, restore the recovery file without asking.
        #[arg(long, conflicts_with = "no_recover")]
        recover: bool,
        /// After a crash, discard the recovery file without asking.
        #[arg(long)]
        no_recover: bool,
    },
    /// Save the current state, close windows in order and stop the context.
    Down {
        #[command(flatten)]
        ctx: Ctx,
        /// State to write (default: the `default` role). Writes only this state.
        #[arg(short, long, conflicts_with = "no_save")]
        state: Option<String>,
        /// Do not save anything; all states stay unchanged.
        #[arg(long)]
        no_save: bool,
    },
    /// Save the current state without shutting down.
    Save {
        #[command(flatten)]
        ctx: Ctx,
        #[arg(short, long)]
        state: Option<String>,
    },
    /// Manage states.
    #[command(subcommand)]
    State(StateCmd),
    /// Switch layout (`koje layout <name>`) or manage layouts.
    #[command(args_conflicts_with_subcommands = true)]
    Layout {
        #[command(subcommand)]
        cmd: Option<LayoutCmd>,
        /// Layout to switch to.
        name: Option<String>,
        #[command(flatten)]
        ctx: ActiveCtx,
    },
    /// Move the focused window to the stash (remove it from the active layout).
    Stash,
    /// Bring a stashed window into the active (or given) layout.
    Unstash {
        window: String,
        #[arg(long)]
        to: Option<String>,
    },
    /// Switch to a context.
    Focus(Ctx),
    /// Place the focused window into a snap zone.
    Snap { zone: String },
    /// Toggle an overlay (e.g. `mail`, `chat`).
    Overlay { name: String },
    /// Pin the focused overlay window onto the current workspace.
    Pin,
    /// Send a pinned window back to its overlay.
    Unpin,
    /// Pause a context (freeze all processes, keep memory).
    Sleep(Ctx),
    /// Resume a paused context.
    Wake(Ctx),
    /// Show memory usage per context.
    Stats { context: Option<String> },
    /// Change memory limits of a running or stopped context.
    Limit {
        #[command(flatten)]
        ctx: Ctx,
        /// Soft limit, e.g. `6G` (warns at 90 % and 100 %).
        #[arg(long)]
        soft: Option<String>,
        /// Hard limit, e.g. `8G` (orderly shutdown).
        #[arg(long)]
        hard: Option<String>,
    },
    /// Open a URL or file inside a context.
    Open {
        #[command(flatten)]
        ctx: Ctx,
        target: String,
    },
    /// Open a shell in a context (or its dev container).
    Shell {
        #[command(flatten)]
        ctx: Ctx,
        #[arg(long)]
        dev: bool,
    },
    /// Create a new context.
    New {
        #[command(flatten)]
        ctx: Ctx,
        /// Border color as #RRGGBB.
        #[arg(long)]
        color: Option<String>,
        /// Short label for bars and menus (default: derived from the name).
        #[arg(long)]
        prefix: Option<String>,
    },
    /// Open a context's configuration in $EDITOR.
    Edit(Ctx),
    /// Remove a context including its containers and states.
    Rm {
        #[command(flatten)]
        ctx: Ctx,
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Manage koje's base image `koje/base`.
    #[command(subcommand)]
    Image(ImageCmd),
    /// List contexts.
    List {
        /// Machine-readable output (JSON array).
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum ImageCmd {
    /// Build `koje/base` for the current user with Podman (a few minutes).
    Build {
        /// Rebuild every layer, e.g. to pick up distribution updates.
        #[arg(long)]
        no_cache: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum StateCmd {
    /// List the states of a context.
    List(Ctx),
    /// Copy a state, e.g. `koje state cp kp default fresh`.
    Cp {
        context: String,
        from: String,
        to: String,
    },
    /// Rename a state.
    Mv {
        context: String,
        from: String,
        to: String,
    },
    /// Delete a state (the only way states are deleted).
    Rm { context: String, state: String },
}

#[derive(Debug, Subcommand)]
pub enum LayoutCmd {
    /// Switch to the next layout.
    Next(ActiveCtx),
    /// Switch to the previous layout.
    Prev(ActiveCtx),
    /// Create a layout (empty, or a copy of the active one).
    New {
        name: String,
        #[arg(long)]
        copy: bool,
        #[command(flatten)]
        ctx: ActiveCtx,
    },
    /// Write the current arrangement into the active layout.
    Save(ActiveCtx),
    /// Turn automatic saving of a layout on or off.
    Autosave {
        name: String,
        #[arg(value_parser = ["on", "off"])]
        value: String,
        #[command(flatten)]
        ctx: ActiveCtx,
    },
    /// List layouts of the active state.
    List(ActiveCtx),
    /// Rename a layout.
    Mv {
        from: String,
        to: String,
        #[command(flatten)]
        ctx: ActiveCtx,
    },
    /// Delete a layout.
    Rm {
        name: String,
        #[command(flatten)]
        ctx: ActiveCtx,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("koje").chain(args.iter().copied()))
    }

    #[test]
    fn definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn down_state_and_no_save_conflict() {
        assert!(parse(&["down", "kp", "-s", "review", "--no-save"]).is_err());
        assert!(parse(&["down", "kp", "--no-save"]).is_ok());
        assert!(parse(&["down", "kp", "-s", "review"]).is_ok());
    }

    #[test]
    fn up_takes_no_save() {
        assert!(matches!(
            parse(&["up", "kp", "-s", "workshop", "--no-save"])
                .unwrap()
                .command,
            Command::Up { no_save: true, .. }
        ));
    }

    #[test]
    fn recover_flags_conflict() {
        assert!(parse(&["up", "kp", "--recover", "--no-recover"]).is_err());
    }

    #[test]
    fn layout_name_or_subcommand() {
        assert!(matches!(
            parse(&["layout", "review"]).unwrap().command,
            Command::Layout { name: Some(n), cmd: None, .. } if n == "review"
        ));
        assert!(matches!(
            parse(&["layout", "next"]).unwrap().command,
            Command::Layout {
                cmd: Some(LayoutCmd::Next(_)),
                ..
            }
        ));
    }

    #[test]
    fn image_build() {
        assert!(matches!(
            parse(&["image", "build", "--no-cache"]).unwrap().command,
            Command::Image(ImageCmd::Build { no_cache: true })
        ));
    }

    #[test]
    fn list_takes_json() {
        assert!(matches!(
            parse(&["list", "--json"]).unwrap().command,
            Command::List { json: true }
        ));
    }

    #[test]
    fn autosave_only_on_or_off() {
        assert!(parse(&["layout", "autosave", "review", "off"]).is_ok());
        assert!(parse(&["layout", "autosave", "review", "maybe"]).is_err());
    }
}
