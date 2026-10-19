//! Talking to kojed over the session bus (ADR 0027). Each call opens its own
//! connection on a small tokio runtime; the CLI stays synchronous.

use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use koje_core::api::{BUS_NAME, INTERFACE, OBJECT_PATH};
use zbus::zvariant::{DynamicType, Type};

const TIMEOUT: Duration = Duration::from_secs(5);

pub const NOT_RUNNING: &str = "kojed is not running – start it with: systemctl --user start kojed";

/// Call a method of `io.koje.Daemon1`.
pub fn call<B, R>(method: &str, body: &B) -> Result<R>
where
    B: serde::Serialize + DynamicType + Sync,
    R: for<'d> serde::Deserialize<'d> + Type,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting the D-Bus client")?;
    runtime.block_on(async {
        tokio::time::timeout(TIMEOUT, async {
            let conn = zbus::Connection::session()
                .await
                .context("connecting to the session bus")?;
            let dbus = zbus::fdo::DBusProxy::new(&conn).await?;
            if !dbus.name_has_owner(BUS_NAME.try_into()?).await? {
                bail!(NOT_RUNNING);
            }
            let reply = conn
                .call_method(Some(BUS_NAME), OBJECT_PATH, Some(INTERFACE), method, body)
                .await
                .with_context(|| format!("kojed: {method}"))?;
            Ok(reply.body().deserialize()?)
        })
        .await
        .with_context(|| format!("kojed did not answer {method} within {TIMEOUT:?}"))?
    })
}

/// Tell a running kojed that contexts changed; without kojed nothing to do
/// (it reads them when it starts).
pub fn reload() {
    if let Err(e) = call::<_, ()>("Reload", &()) {
        tracing::debug!("not reloading kojed: {e:#}");
    }
}
