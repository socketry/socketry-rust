//! Windows socket readiness selector, using `polling`'s IOCP/AFD backend through
//! `async-io`. File operations use the blocking pool. This does not yet submit
//! native overlapped file reads or writes.

pub use super::readiness::{Listener, Selector, Socket};
