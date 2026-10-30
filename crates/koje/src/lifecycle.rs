//! `koje up`, `down`, `save`, `sleep`, `wake` and `recreate` through kojed
//! (G.29, ADR 0060): kojed runs them; the CLI asks what only the person at
//! the terminal can answer and prints the operation's lines as they come.

use std::time::SystemTime;

use anyhow::{Context as _, Result, bail};
use futures_util::StreamExt as _;
use koje_core::name::ContextName;
use koje_core::paths::Paths;
use koje_runtime::kojed::NOT_RUNNING;
use koje_runtime::prompt::confirm;

/// The part of `io.koje.Daemon1` the CLI drives
/// (`crates/kojed/dbus/io.koje.Daemon1.xml`).
#[zbus::proxy(
    interface = "io.koje.Daemon1",
    default_service = "io.koje.Daemon",
    default_path = "/io/koje/Daemon"
)]
trait Daemon1 {
    fn contexts(&self) -> zbus::Result<Vec<(String, String, String, String, String, u32)>>;
    #[allow(clippy::fn_params_excessive_bools)] // the D-Bus signature
    fn up(
        &self,
        context: &str,
        state: &str,
        recovery: &str,
        focus: bool,
        save: bool,
    ) -> zbus::Result<()>;
    fn down(&self, context: &str, state: &str, save: bool, force: bool) -> zbus::Result<()>;
    fn save(&self, context: &str, state: &str) -> zbus::Result<(String, u32)>;
    fn sleep(&self, context: &str) -> zbus::Result<()>;
    fn wake(&self, context: &str) -> zbus::Result<()>;
    fn recreate(&self, context: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    fn operation_progress(&self, context: &str, operation: &str, line: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    fn operation_finished(&self, context: &str, operation: &str) -> zbus::Result<()>;
    #[zbus(signal)]
    fn operation_failed(
        &self,
        context: &str,
        operation: &str,
        error: &str,
        message: &str,
    ) -> zbus::Result<()>;
}

/// `koje up`'s flags.
#[derive(Debug, Clone, Copy)]
#[allow(clippy::struct_excessive_bools)] // independent CLI flags
pub struct Up {
    pub no_save: bool,
    pub recover: bool,
    pub no_recover: bool,
    pub focus: bool,
}

/// How an operation ended.
enum End {
    Finished,
    Failed { error: String, message: String },
}

const WINDOWS_OPEN: &str = "io.koje.Error.WindowsOpen";
const RECOVERY_PENDING: &str = "io.koje.Error.RecoveryPending";

pub fn up(paths: &Paths, context: &str, state: Option<&str>, flags: Up) -> Result<()> {
    let name = ContextName::new(context)?;
    koje_core::context::load(paths, &name)?;
    let interactive = std::io::IsTerminal::is_terminal(&std::io::stdin());
    // Asked here, before kojed starts it: the settings apply only to a new
    // container (M5.11).
    let recreate = interactive
        && koje_runtime::runtime::config_changed(paths, &name)?
        && confirm(
            &format!(
                "{name}'s image or access changed since its container was created. \
                 Recreate it now? Everything installed in it outside your home is lost."
            ),
            false,
        )?;
    let recovery = if flags.recover {
        "restore"
    } else if flags.no_recover {
        "discard"
    } else {
        ""
    };
    let state = state.unwrap_or_default();
    with_kojed(|kojed| async move {
        let (k, n) = (&kojed, &name);
        if recreate {
            operation(k, n, "recreate", || k.recreate(n.as_str()))
                .await
                .map_err(refused)?
                .or_fail()?;
        }
        let up = |recovery: &'static str| {
            operation(k, n, "up", move || {
                k.up(n.as_str(), state, recovery, flags.focus, !flags.no_save)
            })
        };
        match up(recovery).await {
            Err(e) if dbus_error(&e) == Some(RECOVERY_PENDING) => {
                let answer = recovery_answer(paths, n, interactive)?;
                up(answer).await.map_err(refused)?.or_fail()
            }
            other => other.map_err(refused)?.or_fail(),
        }
    })
}

/// After a crash (ADR 0006): asked; nobody to ask (a launcher, autostart)
/// keeps the recovery file for later.
fn recovery_answer(paths: &Paths, name: &ContextName, interactive: bool) -> Result<&'static str> {
    if !interactive {
        return Ok("keep");
    }
    let ago = koje_core::recovery::read(paths, name)?
        .and_then(|r| SystemTime::now().duration_since(r.saved_at).ok())
        .map_or_else(|| "earlier".to_owned(), koje_core::recovery::ago);
    let restore = confirm(
        &format!("{name} did not shut down cleanly – restore its windows from {ago}?"),
        true,
    )?;
    Ok(if restore { "restore" } else { "discard" })
}

pub fn down(paths: &Paths, context: &str, state: Option<&str>, no_save: bool) -> Result<()> {
    let name = ContextName::new(context)?;
    koje_core::context::load(paths, &name)?;
    let state = state.unwrap_or_default();
    with_kojed(|kojed| async move {
        let end = operation(&kojed, &name, "down", || {
            kojed.down(name.as_str(), state, !no_save, false)
        })
        .await
        .map_err(refused)?;
        match end {
            // Saved and closed what it could; the rest may be asking to save.
            End::Failed { error, message } if error == WINDOWS_OPEN => {
                if !std::io::IsTerminal::is_terminal(&std::io::stdin())
                    || !confirm(&format!("Stop {name} anyway?"), false)?
                {
                    bail!("{message}");
                }
                operation(&kojed, &name, "down", || {
                    kojed.down(name.as_str(), "", false, true)
                })
                .await
                .map_err(refused)?
                .or_fail()
            }
            end => end.or_fail(),
        }
    })
}

pub fn save(paths: &Paths, context: &str, state: Option<&str>) -> Result<()> {
    let name = ContextName::new(context)?;
    koje_core::context::load(paths, &name)?;
    with_kojed(|kojed| async move {
        let (saved, windows) = kojed
            .save(name.as_str(), state.unwrap_or_default())
            .await
            .map_err(refused)?;
        println!("saved {windows} window(s) of {name} into state {saved}");
        Ok(())
    })
}

pub fn sleep(paths: &Paths, context: &str) -> Result<()> {
    let name = ContextName::new(context)?;
    koje_core::context::load(paths, &name)?;
    with_kojed(|kojed| async move {
        operation(&kojed, &name, "sleep", || kojed.sleep(name.as_str()))
            .await
            .map_err(refused)?
            .or_fail()
    })
}

pub fn wake(paths: &Paths, context: &str) -> Result<()> {
    let name = ContextName::new(context)?;
    koje_core::context::load(paths, &name)?;
    with_kojed(|kojed| async move {
        let contexts = kojed.contexts().await.map_err(refused)?;
        let status = contexts
            .iter()
            .find(|c| c.0 == name.as_str())
            .map(|c| c.4.as_str());
        match status {
            Some("paused") => {
                kojed.wake(name.as_str()).await.map_err(refused)?;
                println!("{name} is awake");
            }
            Some("running") => println!("{name} is not asleep"),
            _ => bail!("{name} is not running – start it with: koje up {name} -s empty"),
        }
        Ok(())
    })
}

pub fn recreate(paths: &Paths, context: &str, yes: bool) -> Result<()> {
    let name = ContextName::new(context)?;
    koje_core::context::load(paths, &name)?;
    if !yes
        && !confirm(
            &format!(
                "Recreate {name}'s container? Everything installed in it outside your home \
                 is lost; the home, states and config stay."
            ),
            false,
        )?
    {
        bail!("{name} unchanged");
    }
    with_kojed(|kojed| async move {
        operation(&kojed, &name, "recreate", || kojed.recreate(name.as_str()))
            .await
            .map_err(refused)?
            .or_fail()
    })
}

impl End {
    fn or_fail(self) -> Result<()> {
        match self {
            Self::Finished => Ok(()),
            Self::Failed { message, .. } => bail!("{message}"),
        }
    }
}

/// Run `f` with a proxy for a running kojed, on a small tokio runtime; the
/// CLI stays synchronous.
fn with_kojed<F, Fut>(f: F) -> Result<()>
where
    F: FnOnce(Daemon1Proxy<'static>) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting the D-Bus client")?;
    runtime.block_on(async {
        let conn = zbus::Connection::session()
            .await
            .context("connecting to the session bus")?;
        let kojed = Daemon1Proxy::new(&conn).await?;
        let dbus = zbus::fdo::DBusProxy::new(&conn).await?;
        if !dbus
            .name_has_owner(koje_core::api::BUS_NAME.try_into()?)
            .await?
        {
            bail!(NOT_RUNNING);
        }
        f(kojed).await
    })
}

/// Start an operation with `start` and follow it: its lines printed, until
/// it finishes or fails. An error of `start` itself (refused at once) is
/// the `Err`.
async fn operation<Fut>(
    kojed: &Daemon1Proxy<'_>,
    name: &ContextName,
    op: &str,
    start: impl FnOnce() -> Fut,
) -> Result<End, zbus::Error>
where
    Fut: Future<Output = zbus::Result<()>>,
{
    // Subscribed before the call: nothing is missed.
    let mut progress = kojed.receive_operation_progress().await?;
    let mut finished = kojed.receive_operation_finished().await?;
    let mut failed = kojed.receive_operation_failed().await?;
    let mut owner = kojed.inner().receive_owner_changed().await?;
    start().await?;
    let ours = |context: &str, operation: &str| context == name.as_str() && operation == op;
    loop {
        tokio::select! {
            // Lines first: the end comes after the last one.
            biased;
            Some(s) = progress.next() => {
                let a = s.args()?;
                if ours(a.context, a.operation) {
                    println!("{}", a.line);
                }
            }
            Some(s) = finished.next() => {
                let a = s.args()?;
                if ours(a.context, a.operation) {
                    return Ok(End::Finished);
                }
            }
            Some(s) = failed.next() => {
                let a = s.args()?;
                if ours(a.context, a.operation) {
                    return Ok(End::Failed {
                        error: a.error.to_owned(),
                        message: a.message.to_owned(),
                    });
                }
            }
            Some(o) = owner.next() => {
                if o.is_none() {
                    return Ok(End::Failed {
                        error: String::new(),
                        message: format!("kojed stopped during {op} of {name}"),
                    });
                }
            }
        }
    }
}

/// The D-Bus error name of a method's error.
fn dbus_error(e: &zbus::Error) -> Option<&str> {
    match e {
        zbus::Error::MethodError(name, _, _) => Some(name.as_str()),
        _ => None,
    }
}

/// kojed's message for a method's error, without the error name.
fn refused(e: zbus::Error) -> anyhow::Error {
    match e {
        zbus::Error::MethodError(_, Some(m), _) => anyhow::anyhow!(m),
        other => anyhow::Error::new(other).context("kojed"),
    }
}
