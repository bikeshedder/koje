//! kojed's client API on the session bus (ADR 0027): names shared by kojed
//! and its clients.

/// Bus name kojed owns.
pub const BUS_NAME: &str = "io.koje.Daemon";
pub const OBJECT_PATH: &str = "/io/koje/Daemon";
/// Interface, versioned: a breaking change gets `io.koje.Daemon2`.
pub const INTERFACE: &str = "io.koje.Daemon1";
