//! Scheduler implementations and their I/O selectors.

use std::fs::File;
use std::future::Future;
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

pub mod selector;
pub mod socketry;

#[cfg(any(feature = "native", feature = "tokio"))]
mod file;

#[cfg(feature = "tokio")]
pub mod tokio;

pub use socketry::{Scheduler, SchedulerHandle};
pub(crate) use socketry::{Shared, enter};

/// An operation result together with its reusable, owned buffer.
///
/// Reads fill the existing buffer length and leave its length unchanged. Only
/// the first `result?` bytes contain newly read data. Writes may be partial.
/// The buffer is returned on both success and failure.
pub type BufferResult = (io::Result<usize>, Vec<u8>);

/// A socket readiness condition. Readiness can be spurious; retry nonblocking
/// operations and wait again when they return `WouldBlock`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Interest {
    Readable,
    Writable,
}

/// Portable socket operations, selected through the concrete implementation.
///
/// Registrations belong to their creating implementation. Keep a socket's
/// registration across operations and worker migration. Implementations must
/// return an error when a resource belongs to an incompatible runtime instance.
///
/// Dropping an operation future abandons its result. It need not undo an I/O
/// operation already submitted to the kernel: a cancelled read can consume
/// bytes, and a cancelled write can transmit bytes. Implementations retain any
/// kernel-accessible memory until the operation finishes. Await completion when
/// the amount transferred matters. No asynchronous cleanup is promised by Drop.
pub trait Network: Send + Sync {
    type Socket: Send + Sync;
    type Listener: Send + Sync;

    /// Register an owned socket once. The implementation sets nonblocking mode
    /// when required. Do not change its mode through another OS handle.
    fn register_socket(&self, socket: TcpStream) -> io::Result<Self::Socket>;

    /// Register an owned listener once, with the same mode requirements.
    fn register_listener(&self, listener: TcpListener) -> io::Result<Self::Listener>;

    fn connect(&self, address: SocketAddr)
    -> impl Future<Output = io::Result<Self::Socket>> + Send;

    fn accept(
        &self,
        listener: &Self::Listener,
    ) -> impl Future<Output = io::Result<(Self::Socket, SocketAddr)>> + Send;

    fn io_read(
        &self,
        socket: &Self::Socket,
        buffer: Vec<u8>,
    ) -> impl Future<Output = BufferResult> + Send;

    fn io_write(
        &self,
        socket: &Self::Socket,
        buffer: Vec<u8>,
    ) -> impl Future<Output = BufferResult> + Send;

    fn io_wait(
        &self,
        socket: &Self::Socket,
        interest: Interest,
    ) -> impl Future<Output = io::Result<()>> + Send;
}

/// Positioned file operations. A regular file does not support a universal
/// readiness fallback, so implementations use native completion or a blocking
/// pool. Use ordinary files opened without append mode, not pipes. Offsets
/// must fit in i64. The Unix implementation leaves the shared cursor unchanged;
/// the Windows blocking fallback updates it, as std's seek_read/seek_write do.
///
/// Buffers and the file remain owned by an in-flight operation even if the
/// waiting future is dropped. A write can still complete after cancellation.
pub trait FileIo: Send + Sync {
    fn file_read_at(
        &self,
        file: Arc<File>,
        buffer: Vec<u8>,
        offset: u64,
    ) -> impl Future<Output = BufferResult> + Send;

    fn file_write_at(
        &self,
        file: Arc<File>,
        buffer: Vec<u8>,
        offset: u64,
    ) -> impl Future<Output = BufferResult> + Send;
}

/// A runtime's monotonic sleep facility.
pub trait Clock: Send + Sync {
    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> + Send;
}
