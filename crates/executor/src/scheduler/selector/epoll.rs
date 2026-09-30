//! Linux/Android readiness selector. `async-io` uses `polling`'s epoll backend.
//! Reads and writes share the nonblocking readiness fallback.

pub use super::readiness::{Listener, Selector, Socket};
