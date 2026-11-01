//! SHAG `src/engine/network/client`: the client's side of a game server
//! connection. [`network_session`] is the I/O the browser's event loop and
//! WebSocket provide in SHAG.

pub mod clock_sync;
pub mod connection_watchdog;
pub mod hosts;
pub mod network_session;
pub mod snapshot_buffer;
pub mod snapshot_receiver;
pub mod tls;
