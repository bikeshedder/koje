//! `koje-forward` – runs inside a koje context (ADR 0019, 0025).
//!
//!   koje-forward --host <address> <name>…
//!   koje-forward --host unix:path=/run/user/1000/koje-host/bus \
//!       org.freedesktop.Notifications org.freedesktop.portal.Desktop
//!
//! Owns the names on the context's session bus (`$DBUS_SESSION_BUS_ADDRESS`) and
//! forwards their calls to the filtered host bus proxy. Exits when the context
//! bus goes away. Built statically (musl) and mounted into every context.

mod forward;
mod translate;

use anyhow::{Context as _, Result, bail};
use tracing_subscriber::EnvFilter;

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<(String, Vec<String>)> {
    let mut host = None;
    let mut names = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--host" => host = Some(args.next().context("--host needs an address")?),
            "-h" | "--help" => bail!("usage: koje-forward --host <address> <name>…"),
            name => names.push(name.to_owned()),
        }
    }
    let host = host.context("--host <address> is required")?;
    if names.is_empty() {
        bail!("no names to forward");
    }
    Ok((host, names))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("KOJE_FORWARD_LOG")
                .unwrap_or_else(|_| EnvFilter::new("warn,zbus=error")),
        )
        .with_writer(std::io::stderr)
        .init();
    let (host, names) = parse_args(std::env::args().skip(1))?;
    let ctx = zbus::Connection::session()
        .await
        .context("connecting to the context bus")?;
    let forwarder = forward::Forwarder::new(ctx, host, names).await?;
    forwarder.run().await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Result<(String, Vec<String>)> {
        parse_args(a.iter().map(|s| (*s).to_owned()))
    }

    #[test]
    fn arguments() {
        let (host, names) = args(&["--host", "unix:path=/x", "a.b", "c.d"]).unwrap();
        assert_eq!(host, "unix:path=/x");
        assert_eq!(names, ["a.b", "c.d"]);
        assert!(args(&["a.b"]).is_err());
        assert!(args(&["--host", "x"]).is_err());
    }
}
