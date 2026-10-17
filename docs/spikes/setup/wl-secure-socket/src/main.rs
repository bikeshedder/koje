//! M2.9 spike: a Wayland socket for one context via `wp_security_context_v1`.
//!
//!   wl-secure-socket <socket-path> <context-name>
//!
//! Connects to the compositor ($WAYLAND_DISPLAY), binds a new listening socket
//! at <socket-path> and registers it as a security context (sandbox engine
//! `io.koje`, app id `io.koje.<context-name>`). Clients connecting to that
//! socket are known to the compositor as sandboxed. Runs until Ctrl+C; the
//! compositor stops accepting on the socket when this process exits.

use std::os::fd::AsFd;
use std::os::unix::net::UnixListener;

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::wp::security_context::v1::client::{
    wp_security_context_manager_v1::WpSecurityContextManagerV1,
    wp_security_context_v1::WpSecurityContextV1,
};

struct State;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<WpSecurityContextManagerV1, ()> for State {
    fn event(_: &mut Self, _: &WpSecurityContextManagerV1, _: <WpSecurityContextManagerV1 as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<WpSecurityContextV1, ()> for State {
    fn event(_: &mut Self, _: &WpSecurityContextV1, _: <WpSecurityContextV1 as wayland_client::Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [path, context] = args.as_slice() else {
        return Err("usage: wl-secure-socket <socket-path> <context-name>".into());
    };

    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn)?;
    let qh = queue.handle();
    let manager: WpSecurityContextManagerV1 = globals
        .bind(&qh, 1..=1, ())
        .map_err(|e| format!("compositor does not offer wp_security_context_manager_v1: {e}"))?;

    let _ = std::fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    // The compositor watches the read end; it stops serving the socket once
    // this process (holding the write end) exits.
    let (close_read, _close_write) = std::io::pipe()?;

    let security_context = manager.create_listener(listener.as_fd(), close_read.as_fd(), &qh, ());
    security_context.set_sandbox_engine("io.koje".into());
    security_context.set_app_id(format!("io.koje.{context}"));
    security_context.set_instance_id(context.clone());
    security_context.commit();
    queue.roundtrip(&mut State)?;

    println!("security context for {context:?} listening on {path} (Ctrl+C to stop)");
    loop {
        queue.blocking_dispatch(&mut State)?;
    }
}
