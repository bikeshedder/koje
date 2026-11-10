//! Command line definition. Mirrors the command table in the concept
//! (`docs/concept/koje-konzept.pdf`, section "Das Werkzeug: koje") and
//! `docs/decisions/0005-named-states-and-cli.md`. Change both together.

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "koje", version, about = "Isolated work contexts on GNOME", long_about = None)]
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

/// Optional context; defaults to the active context.
#[derive(Debug, Args)]
pub struct ActiveCtx {
    /// Context to act on (default: the active one – the context of the focused
    /// workspace, else of the focused window).
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
        /// into the state it is in first.
        #[arg(long)]
        no_save: bool,
        /// After a crash, restore the recovery file without asking.
        #[arg(long, conflicts_with = "no_recover")]
        recover: bool,
        /// After a crash, discard the recovery file without asking.
        #[arg(long)]
        no_recover: bool,
        /// Stay on the current workspace (default: switch to the context's
        /// before its programs start).
        #[arg(long)]
        no_focus: bool,
    },
    /// Save the current state, close windows in order and stop the context.
    Down {
        #[command(flatten)]
        ctx: Ctx,
        /// State to write (default: the state the context is in, else `default`).
        /// Writes only this state.
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
        /// State to write (default: the state the context is in, else `default`).
        #[arg(short, long)]
        state: Option<String>,
    },
    /// Manage states.
    #[command(subcommand)]
    State(StateCmd),
    /// Switch to a context.
    Focus(Ctx),
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
        /// Soft limit, e.g. `6G` (warns at 90 % and 100 %); `none` removes it.
        #[arg(long)]
        soft: Option<String>,
        /// Hard limit, e.g. `8G` (orderly shutdown); `none` removes it.
        #[arg(long)]
        hard: Option<String>,
    },
    /// Open a URL or file inside a context.
    Open {
        #[command(flatten)]
        ctx: Ctx,
        target: String,
    },
    /// Start a program in a running context, detached (e.g. `koje run kp foot`).
    /// It starts in the current directory if the context mounts it.
    Run {
        #[command(flatten)]
        ctx: Ctx,
        /// Stay on the current workspace (default: switch to the context's
        /// first, so the window opens where you look).
        #[arg(long)]
        no_focus: bool,
        /// Run it as this user in the context (e.g. `root`; default: yours).
        #[arg(short, long)]
        user: Option<String>,
        /// The program and its arguments, passed as they are (no shell).
        #[arg(
            required = true,
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "PROGRAM"
        )]
        command: Vec<String>,
    },
    /// List the programs of the active context, or start one.
    Apps {
        #[command(flatten)]
        ctx: ActiveCtx,
        /// Start the program with this name (as listed) or desktop file id
        /// (`firefox.desktop`) instead.
        #[arg(long, value_name = "NAME")]
        start: Option<String>,
    },
    /// The dash's favorites in a context (ADR 0053): list them, or set them.
    /// Unset, GNOME's favorites apply as far as the context has them.
    Favorites {
        #[command(flatten)]
        ctx: ActiveCtx,
        /// Set them to these desktop file ids of the context, in this order;
        /// none: no favorites.
        #[arg(long, num_args = 0.., value_name = "ID", conflicts_with = "reset")]
        set: Option<Vec<String>>,
        /// Remove them: GNOME's favorites apply again.
        #[arg(long)]
        reset: bool,
    },
    /// Link the dotfiles presets again now (e.g. after adding files to one) in
    /// every running context, or one; lists the presets there are (ADR 0038).
    Dotfiles {
        /// Only this context (default: all running ones).
        #[arg(short, long)]
        context: Option<String>,
    },
    /// Open a terminal in the active context (Super+Return); on a workspace
    /// without a context, on the host.
    Terminal(ActiveCtx),
    /// Open a shell in a context (or its dev container).
    Shell {
        #[command(flatten)]
        ctx: Ctx,
        /// Enter a dev container instead: the only running one, or the named one.
        #[arg(long, value_name = "CONTAINER", num_args = 0..=1, default_missing_value = "")]
        dev: Option<String>,
        /// Log in as this user (e.g. `root`; default: yours). The context is
        /// rootless: its root is your user on the host.
        #[arg(short, long)]
        user: Option<String>,
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
    /// Open koje's settings app, on a context if one is given.
    Settings {
        /// The context to show.
        context: Option<String>,
    },
    /// Open a context's configuration in $EDITOR.
    Edit {
        #[command(flatten)]
        ctx: Ctx,
        /// Take the whole new `context.toml` from stdin instead (the
        /// settings app): checked the same way, nothing changes when it is
        /// invalid.
        #[arg(long)]
        stdin: bool,
    },
    /// Open koje's `config.toml` in $EDITOR (settings for all contexts, the
    /// bar, host workspaces and overlays).
    Config {
        /// Take the whole new `config.toml` from stdin instead (the settings
        /// app): checked the same way, nothing changes when it is invalid.
        #[arg(long)]
        stdin: bool,
    },
    /// Make a new container from the current config (saves and restores the
    /// windows of a running context). Everything installed in the old one
    /// outside the home is lost.
    Recreate {
        #[command(flatten)]
        ctx: Ctx,
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Remove a context including its containers and states.
    Rm {
        #[command(flatten)]
        ctx: Ctx,
        /// Do not ask for confirmation.
        #[arg(long)]
        yes: bool,
    },
    /// Build and create images (ADR 0040): koje's built-in `fedora`, image
    /// folders in ~/.config/koje/images, registry images.
    #[command(subcommand)]
    Image(ImageCmd),
    /// List contexts.
    List,
    /// Install koje's files for this user (Hyprland snippet, Waybar if you
    /// have none, base image) and remove the old install.sh's; run after
    /// every package install or upgrade. Shows the plan first.
    Setup {
        /// Only show the plan.
        #[arg(long)]
        dry_run: bool,
        /// Do not ask.
        #[arg(long, short)]
        yes: bool,
    },
    /// Check what koje needs on this host and how to fix what is missing.
    Doctor,
}

#[derive(Debug, Subcommand)]
pub enum ImageCmd {
    /// Build images with what they build on and your user on top (default:
    /// the images contexts use). Contexts get a new build when recreated.
    Build {
        /// Images as `image = …` names them: `fedora`, a folder's name or
        /// path, a registry image.
        images: Vec<String>,
        /// Rebuild every layer, e.g. to pick up distribution updates.
        #[arg(long)]
        no_cache: bool,
        /// Pull newer base and registry images.
        #[arg(long)]
        pull: bool,
    },
    /// Create the image folder ~/.config/koje/images/<name> with a
    /// Containerfile; use it with `image = "<name>"` in a context.
    New {
        name: String,
        /// The image it builds on (default: fedora).
        #[arg(long, default_value = koje_core::image::DEFAULT)]
        from: String,
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
    fn favorites_list_set_or_reset() {
        assert!(matches!(
            parse(&["favorites"]).unwrap().command,
            Command::Favorites {
                set: None,
                reset: false,
                ..
            }
        ));
        assert!(matches!(
            parse(&["favorites", "-c", "kp", "--set", "a.desktop", "b.desktop"]).unwrap().command,
            Command::Favorites { set: Some(ids), .. } if ids.len() == 2
        ));
        assert!(matches!(
            parse(&["favorites", "--set"]).unwrap().command,
            Command::Favorites { set: Some(ids), .. } if ids.is_empty()
        ));
        assert!(parse(&["favorites", "--set", "a.desktop", "--reset"]).is_err());
    }

    #[test]
    fn apps_lists_or_starts() {
        assert!(matches!(
            parse(&["apps"]).unwrap().command,
            Command::Apps { start: None, .. }
        ));
        assert!(matches!(
            parse(&["apps", "-c", "kp", "--start", "Files"]).unwrap().command,
            Command::Apps { start: Some(s), ctx: ActiveCtx { context: Some(c) }, .. } if s == "Files" && c == "kp"
        ));
    }

    #[test]
    fn dotfiles_all_or_one() {
        assert!(matches!(
            parse(&["dotfiles"]).unwrap().command,
            Command::Dotfiles { context: None }
        ));
        assert!(matches!(
            parse(&["dotfiles", "-c", "kp"]).unwrap().command,
            Command::Dotfiles { context: Some(c) } if c == "kp"
        ));
    }

    #[test]
    fn terminal_context_is_optional() {
        assert!(matches!(
            parse(&["terminal"]).unwrap().command,
            Command::Terminal(ActiveCtx { context: None })
        ));
        assert!(matches!(
            parse(&["terminal", "-c", "kp"]).unwrap().command,
            Command::Terminal(ActiveCtx { context: Some(c) }) if c == "kp"
        ));
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
    fn no_focus_for_up_and_run() {
        assert!(matches!(
            parse(&["up", "kp", "--no-focus"]).unwrap().command,
            Command::Up { no_focus: true, .. }
        ));
        match parse(&["run", "kp", "--no-focus", "foot", "--app-id=x"])
            .unwrap()
            .command
        {
            Command::Run {
                no_focus, command, ..
            } => {
                assert!(no_focus);
                assert_eq!(
                    command,
                    ["foot", "--app-id=x"],
                    "the program's own flags stay its own"
                );
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            parse(&["run", "kp", "foot", "--no-focus"]).unwrap().command,
            Command::Run {
                no_focus: false,
                ..
            }
        ));
    }

    #[test]
    fn recover_flags_conflict() {
        assert!(parse(&["up", "kp", "--recover", "--no-recover"]).is_err());
    }

    #[test]
    fn shell_dev_takes_an_optional_name() {
        let dev = |args: &[&str]| match parse(args).unwrap().command {
            Command::Shell { dev, .. } => dev,
            _ => unreachable!(),
        };
        assert_eq!(dev(&["shell", "kp"]), None);
        assert_eq!(dev(&["shell", "kp", "--dev"]), Some(String::new()));
        assert_eq!(dev(&["shell", "kp", "--dev", "web"]), Some("web".into()));
    }

    #[test]
    fn shell_and_run_take_a_user() {
        assert!(matches!(
            parse(&["shell", "kp", "-u", "root"]).unwrap().command,
            Command::Shell { user: Some(u), .. } if u == "root"
        ));
        let Command::Run { user, command, .. } =
            parse(&["run", "kp", "--user", "root", "dnf", "-y", "upgrade"])
                .unwrap()
                .command
        else {
            unreachable!()
        };
        assert_eq!(user.as_deref(), Some("root"));
        assert_eq!(command, ["dnf", "-y", "upgrade"]);
        // After the program, `-u` is the program's.
        let Command::Run { user, command, .. } =
            parse(&["run", "kp", "sort", "-u"]).unwrap().command
        else {
            unreachable!()
        };
        assert_eq!(user, None);
        assert_eq!(command, ["sort", "-u"]);
    }

    #[test]
    fn image_build() {
        assert!(matches!(
            parse(&["image", "build", "--no-cache"]).unwrap().command,
            Command::Image(ImageCmd::Build { no_cache: true, .. })
        ));
    }

    #[test]
    fn list_takes_no_json() {
        assert!(matches!(parse(&["list"]).unwrap().command, Command::List));
        // ADR 0060: scripts use D-Bus (busctl), not --json.
        assert!(parse(&["list", "--json"]).is_err());
        assert!(parse(&["stats", "--json"]).is_err());
    }
}
