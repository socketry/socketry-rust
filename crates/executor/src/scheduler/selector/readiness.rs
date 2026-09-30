//! Shared readiness operations backed by `async-io`'s persistent registrations.
//!
//! Its process-wide reactor drives OS events independently of our executor.
//! It remains available while registered resources are alive; Socketry task
//! shutdown does not shut down that shared reactor. Futures contain no private
//! coroutine stacks and may be polled on different workers.

use crate::scheduler::file::{read_at, write_at};
use crate::scheduler::{BufferResult, Clock, FileIo, Interest, Network};
use async_io::Async;
use std::fs::File;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

/// A reusable socket registration. Clones share one underlying registration.
#[derive(Clone)]
pub struct Socket(pub(crate) Arc<Async<TcpStream>>);

impl Socket {
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.get_ref().local_addr()
    }

    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.0.get_ref().peer_addr()
    }
}

/// A reusable listening socket registration.
pub struct Listener(Async<TcpListener>);

impl Listener {
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.get_ref().local_addr()
    }
}

/// Compile-time platform selection with a shared readiness reactor.
/// All instances share the reactor and may use each other's registrations.
#[derive(Clone, Copy, Default)]
pub struct Selector;

impl Selector {
    pub fn new() -> io::Result<Self> {
        Ok(Self)
    }
}

impl Network for Selector {
    type Socket = Socket;
    type Listener = Listener;

    fn register_socket(&self, socket: TcpStream) -> io::Result<Socket> {
        Ok(Socket(Arc::new(Async::new(socket)?)))
    }

    fn register_listener(&self, listener: TcpListener) -> io::Result<Listener> {
        Ok(Listener(Async::new(listener)?))
    }

    async fn connect(&self, address: SocketAddr) -> io::Result<Socket> {
        Ok(Socket(Arc::new(
            Async::<TcpStream>::connect(address).await?,
        )))
    }

    async fn accept(&self, listener: &Listener) -> io::Result<(Socket, SocketAddr)> {
        let (socket, address) = listener.0.accept().await?;
        Ok((Socket(Arc::new(socket)), address))
    }

    async fn io_read(&self, socket: &Socket, mut buffer: Vec<u8>) -> BufferResult {
        let result = socket
            .0
            .read_with(|mut socket| socket.read(&mut buffer))
            .await;
        (result, buffer)
    }

    async fn io_write(&self, socket: &Socket, buffer: Vec<u8>) -> BufferResult {
        let result = socket
            .0
            .write_with(|mut socket| socket.write(&buffer))
            .await;
        (result, buffer)
    }

    async fn io_wait(&self, socket: &Socket, interest: Interest) -> io::Result<()> {
        match interest {
            Interest::Readable => socket.0.readable().await,
            Interest::Writable => socket.0.writable().await,
        }
    }
}

impl FileIo for Selector {
    async fn file_read_at(
        &self,
        file: Arc<File>,
        mut buffer: Vec<u8>,
        offset: u64,
    ) -> BufferResult {
        blocking::unblock(move || {
            let result = read_at(&file, &mut buffer, offset);
            (result, buffer)
        })
        .await
    }

    async fn file_write_at(&self, file: Arc<File>, buffer: Vec<u8>, offset: u64) -> BufferResult {
        blocking::unblock(move || {
            let result = write_at(&file, &buffer, offset);
            (result, buffer)
        })
        .await
    }
}

impl Clock for Selector {
    async fn sleep(&self, duration: Duration) {
        async_io::Timer::after(duration).await;
    }
}
