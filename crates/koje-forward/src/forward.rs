//! The forwarder: owns names on the context bus and passes every method call
//! for them to the host bus, over one host connection per calling client
//! (ADR 0019/0025, design from M2.10).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Context as _, Result};
use futures_util::StreamExt as _;
use tokio::sync::oneshot;
use zbus::message::{Builder, Flags, Header, Type};
use zbus::{Connection, MatchRule, Message, MessageStream};

use crate::translate::{PathMap, rewrite_body};

/// One client's connection to the host bus.
struct Upstream {
    conn: Connection,
    to_host: PathMap,
    to_ctx: PathMap,
    /// Calls waiting for their reply, by the serial they were sent with.
    pending: Mutex<HashMap<u32, oneshot::Sender<Message>>>,
}

pub struct Forwarder {
    ctx: Connection,
    host_address: String,
    names: Vec<String>,
    clients: Mutex<HashMap<String, Arc<Upstream>>>,
}

/// Build a message with `body` from `src`, its portal paths mapped.
fn with_body(b: Builder<'_>, src: &Message, map: &PathMap) -> zbus::Result<Message> {
    match rewrite_body(&src.body(), map)? {
        None => b.build(&()),
        Some(fields) => b.build(&fields),
    }
}

impl Forwarder {
    pub async fn new(
        ctx: Connection,
        host_address: String,
        names: Vec<String>,
    ) -> Result<Arc<Self>> {
        for name in &names {
            ctx.request_name(name.as_str())
                .await
                .with_context(|| format!("owning {name} on the context bus"))?;
        }
        Ok(Arc::new(Self {
            ctx,
            host_address,
            names,
            clients: Mutex::new(HashMap::new()),
        }))
    }

    /// Serve until the context bus goes away.
    pub async fn run(self: Arc<Self>) -> Result<()> {
        let gone = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender("org.freedesktop.DBus")?
            .interface("org.freedesktop.DBus")?
            .member("NameOwnerChanged")?
            .build();
        zbus::fdo::DBusProxy::new(&self.ctx)
            .await?
            .add_match_rule(gone)
            .await?;
        let mut stream = MessageStream::from(&self.ctx);
        while let Some(msg) = stream.next().await {
            let msg = msg?;
            match msg.message_type() {
                Type::MethodCall if self.is_for_us(&msg.header()) => {
                    let this = Arc::clone(&self);
                    tokio::spawn(async move {
                        if let Err(e) = this.forward_call(&msg).await {
                            tracing::warn!("forwarding failed: {e:#}");
                            let _ = this.reply_error(&msg, &format!("{e:#}")).await;
                        }
                    });
                }
                Type::Signal => self.client_gone(&msg),
                _ => {}
            }
        }
        Ok(())
    }

    fn is_for_us(&self, h: &Header<'_>) -> bool {
        let dest = h.destination().map(zbus::names::BusName::as_str);
        dest.is_some_and(|d| self.names.iter().any(|n| n == d))
            || dest == self.ctx.unique_name().map(|u| u.as_str())
    }

    /// A client left the context bus: close its host connection, so the host
    /// service releases what it held (inhibits, screen cast sessions).
    fn client_gone(&self, msg: &Message) {
        let h = msg.header();
        if h.member().map(zbus::names::MemberName::as_str) != Some("NameOwnerChanged") {
            return;
        }
        let Ok((name, _old, new)) = msg.body().deserialize::<(String, String, String)>() else {
            return;
        };
        if new.is_empty()
            && let Some(up) = self.lock_clients().remove(&name)
        {
            tracing::debug!(client = %name, "client gone, closing its host connection");
            // close(), not graceful_shutdown(): the reader task holds a stream
            // on this connection, which graceful_shutdown would wait for.
            let conn = up.conn.clone();
            tokio::spawn(async move { conn.close().await });
        }
    }

    #[allow(clippy::unwrap_used)] // a poisoned lock means a panicked task: give up
    fn lock_clients(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<Upstream>>> {
        self.clients.lock().unwrap()
    }

    /// The client's host connection, created on its first call.
    async fn upstream(self: &Arc<Self>, client: &str) -> Result<Arc<Upstream>> {
        if let Some(up) = self
            .lock_clients()
            .get(client)
            .filter(|u| !u.conn.is_closed())
        {
            return Ok(Arc::clone(up));
        }
        let conn = zbus::connection::Builder::address(self.host_address.as_str())?
            .build()
            .await
            .context("connecting to the host bus proxy")?;
        let host_name = conn
            .unique_name()
            .map(ToString::to_string)
            .unwrap_or_default();
        let dbus = zbus::fdo::DBusProxy::new(&conn).await?;
        for name in &self.names {
            let rule = MatchRule::builder()
                .msg_type(Type::Signal)
                .sender(name.as_str())?
                .build();
            dbus.add_match_rule(rule).await?;
        }
        let up = Arc::new(Upstream {
            to_host: PathMap::new(client, &host_name),
            to_ctx: PathMap::new(&host_name, client),
            conn,
            pending: Mutex::new(HashMap::new()),
        });
        tracing::debug!(client, host = %host_name, "new host connection");
        self.lock_clients()
            .insert(client.to_owned(), Arc::clone(&up));
        // The stream must exist before the first call is sent: a stream only
        // sees messages that arrive after it was created.
        let stream = MessageStream::from(&up.conn);
        let this = Arc::clone(self);
        let reader = Arc::clone(&up);
        let client = client.to_owned();
        tokio::spawn(async move { this.read_host(&reader, stream, &client).await });
        Ok(up)
    }

    /// Replies go to the waiting call; signals of the forwarded services go to
    /// the client. Ends when the host connection closes (proxy restart).
    async fn read_host(&self, up: &Upstream, mut stream: MessageStream, client: &str) {
        let mut owners: HashMap<String, String> = HashMap::new();
        while let Some(Ok(msg)) = stream.next().await {
            let h = msg.header();
            tracing::trace!(kind = ?msg.message_type(), reply_serial = ?h.reply_serial(), sender = ?h.sender(), "host message");
            match msg.message_type() {
                Type::MethodReturn | Type::Error => {
                    if msg.message_type() == Type::Error {
                        let text = msg
                            .body()
                            .deserialize::<(String,)>()
                            .map(|(t,)| t)
                            .unwrap_or_default();
                        tracing::debug!(error = ?h.error_name().map(|e| e.as_str()), text, sender = ?h.sender(), "error from the host");
                    }
                    let waiting = h.reply_serial().and_then(|s| {
                        #[allow(clippy::unwrap_used)]
                        up.pending.lock().unwrap().remove(&s.get())
                    });
                    if let Some(tx) = waiting {
                        let _ = tx.send(msg.clone());
                    }
                }
                Type::Signal => {
                    let sender = h.sender().map(ToString::to_string).unwrap_or_default();
                    if !self.is_service(&up.conn, &sender, &mut owners).await {
                        continue;
                    }
                    if let Err(e) = self.forward_signal(&msg, up, client).await {
                        tracing::warn!("forwarding a signal failed: {e:#}");
                    }
                }
                Type::MethodCall => {}
            }
        }
        // Drop it from the map, so the next call opens a new connection.
        let mut clients = self.lock_clients();
        if clients
            .get(client)
            .is_some_and(|u| std::ptr::eq(Arc::as_ptr(u), up))
        {
            clients.remove(client);
        }
    }

    /// Whether `sender` (a unique name) owns one of the forwarded names.
    async fn is_service(
        &self,
        conn: &Connection,
        sender: &str,
        owners: &mut HashMap<String, String>,
    ) -> bool {
        if owners.values().any(|o| o == sender) {
            return true;
        }
        let Ok(dbus) = zbus::fdo::DBusProxy::new(conn).await else {
            return false;
        };
        for name in &self.names {
            let Ok(bus_name) = zbus::names::BusName::try_from(name.as_str()) else {
                continue;
            };
            if let Ok(owner) = dbus.get_name_owner(bus_name).await {
                owners.insert(name.clone(), owner.to_string());
            }
        }
        owners.values().any(|o| o == sender)
    }

    async fn forward_call(self: &Arc<Self>, call: &Message) -> Result<()> {
        let h = call.header();
        let client = h.sender().context("call without sender")?.to_string();
        let dest = match h.destination().map(zbus::names::BusName::as_str) {
            Some(d) if self.names.iter().any(|n| n == d) => d.to_owned(),
            // Called by our unique name: the first forwarded name.
            _ => self.names.first().cloned().context("no names to forward")?,
        };
        let up = self.upstream(&client).await?;
        let path = up
            .to_host
            .path(h.path().context("call without path")?.as_str());
        let mut b = Message::method_call(
            path.as_str(),
            h.member().context("call without member")?.as_str(),
        )?
        .destination(dest.as_str())?;
        if let Some(iface) = h.interface() {
            b = b.interface(iface.as_str())?;
        }
        let no_reply = call
            .primary_header()
            .flags()
            .contains(Flags::NoReplyExpected);
        if no_reply {
            b = b.with_flags(Flags::NoReplyExpected)?;
        }
        let out = with_body(b, call, &up.to_host)?;
        if no_reply {
            up.conn.send(&out).await?;
            return Ok(());
        }
        let (tx, rx) = oneshot::channel();
        #[allow(clippy::unwrap_used)]
        up.pending
            .lock()
            .unwrap()
            .insert(out.primary_header().serial_num().get(), tx);
        up.conn.send(&out).await?;
        tracing::trace!(serial = out.primary_header().serial_num().get(), member = ?h.member(), "call sent to host");
        let reply = rx
            .await
            .context("host connection closed before the reply")?;
        let error_name = reply.header().error_name().map(ToString::to_string);
        let b = match (reply.message_type(), &error_name) {
            (Type::Error, Some(name)) => Message::error(&h, name.as_str())?,
            (Type::Error, None) => anyhow::bail!("error reply without a name"),
            _ => Message::method_return(&h)?,
        };
        let back = with_body(b, &reply, &up.to_ctx)?;
        self.ctx.send(&back).await?;
        Ok(())
    }

    async fn forward_signal(&self, sig: &Message, up: &Upstream, client: &str) -> Result<()> {
        let h = sig.header();
        let path = up
            .to_ctx
            .path(h.path().context("signal without path")?.as_str());
        let b = Message::signal(
            path.as_str(),
            h.interface().context("signal without interface")?.as_str(),
            h.member().context("signal without member")?.as_str(),
        )?
        .destination(client)?;
        self.ctx.send(&with_body(b, sig, &up.to_ctx)?).await?;
        Ok(())
    }

    async fn reply_error(&self, call: &Message, text: &str) -> Result<()> {
        if call
            .primary_header()
            .flags()
            .contains(Flags::NoReplyExpected)
        {
            return Ok(());
        }
        let err =
            Message::error(&call.header(), "org.freedesktop.DBus.Error.Failed")?.build(&(text,))?;
        self.ctx.send(&err).await?;
        Ok(())
    }
}
