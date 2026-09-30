//! Native selector operations exposed by Socketry schedulers and handles.
use super::{Scheduler, SchedulerHandle};
use crate::scheduler::selector::DefaultSelector;
use crate::scheduler::{BufferResult, Clock, FileIo, Interest, Network};
use std::fs::File;
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

impl SchedulerHandle {
    /// Lazily initialize this scheduler's compile-time selected I/O selector.
    pub fn selector(&self) -> io::Result<&DefaultSelector> {
        if self
            .shared
            .closed
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "scheduler is closed",
            ));
        }
        let selector = self
            .shared
            .selector
            .get_or_init(DefaultSelector::new)
            .as_ref()
            .map_err(|error| io::Error::new(error.kind(), error.to_string()))?;
        // Shutdown may have raced lazy initialization. Do not leave a newly
        // constructed completion selector running after admission closes.
        if self
            .shared
            .closed
            .load(std::sync::atomic::Ordering::Acquire)
        {
            #[cfg(all(feature = "io-uring", target_os = "linux"))]
            selector.close();
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "scheduler is closed",
            ));
        }
        Ok(selector)
    }
}

impl Network for SchedulerHandle {
    type Socket = <DefaultSelector as Network>::Socket;
    type Listener = <DefaultSelector as Network>::Listener;

    fn register_socket(&self, socket: TcpStream) -> io::Result<Self::Socket> {
        self.selector()?.register_socket(socket)
    }

    fn register_listener(&self, listener: TcpListener) -> io::Result<Self::Listener> {
        self.selector()?.register_listener(listener)
    }

    async fn connect(&self, address: SocketAddr) -> io::Result<Self::Socket> {
        self.selector()?.connect(address).await
    }

    async fn accept(&self, listener: &Self::Listener) -> io::Result<(Self::Socket, SocketAddr)> {
        self.selector()?.accept(listener).await
    }

    async fn io_read(&self, socket: &Self::Socket, buffer: Vec<u8>) -> BufferResult {
        match self.selector() {
            Ok(selector) => selector.io_read(socket, buffer).await,
            Err(error) => (Err(error), buffer),
        }
    }

    async fn io_write(&self, socket: &Self::Socket, buffer: Vec<u8>) -> BufferResult {
        match self.selector() {
            Ok(selector) => selector.io_write(socket, buffer).await,
            Err(error) => (Err(error), buffer),
        }
    }

    async fn io_wait(&self, socket: &Self::Socket, interest: Interest) -> io::Result<()> {
        self.selector()?.io_wait(socket, interest).await
    }
}

impl FileIo for SchedulerHandle {
    async fn file_read_at(&self, file: Arc<File>, buffer: Vec<u8>, offset: u64) -> BufferResult {
        match self.selector() {
            Ok(selector) => selector.file_read_at(file, buffer, offset).await,
            Err(error) => (Err(error), buffer),
        }
    }

    async fn file_write_at(&self, file: Arc<File>, buffer: Vec<u8>, offset: u64) -> BufferResult {
        match self.selector() {
            Ok(selector) => selector.file_write_at(file, buffer, offset).await,
            Err(error) => (Err(error), buffer),
        }
    }
}

impl Clock for SchedulerHandle {
    async fn sleep(&self, duration: Duration) {
        // Timer progress is independent of selector initialization and ring support.
        async_io::Timer::after(duration).await;
    }
}

impl Network for Scheduler {
    type Socket = <SchedulerHandle as Network>::Socket;
    type Listener = <SchedulerHandle as Network>::Listener;

    fn register_socket(&self, socket: TcpStream) -> io::Result<Self::Socket> {
        self.handle.register_socket(socket)
    }

    fn register_listener(&self, listener: TcpListener) -> io::Result<Self::Listener> {
        self.handle.register_listener(listener)
    }

    async fn connect(&self, address: SocketAddr) -> io::Result<Self::Socket> {
        self.handle.connect(address).await
    }

    async fn accept(&self, listener: &Self::Listener) -> io::Result<(Self::Socket, SocketAddr)> {
        self.handle.accept(listener).await
    }

    async fn io_read(&self, socket: &Self::Socket, buffer: Vec<u8>) -> BufferResult {
        self.handle.io_read(socket, buffer).await
    }

    async fn io_write(&self, socket: &Self::Socket, buffer: Vec<u8>) -> BufferResult {
        self.handle.io_write(socket, buffer).await
    }

    async fn io_wait(&self, socket: &Self::Socket, interest: Interest) -> io::Result<()> {
        self.handle.io_wait(socket, interest).await
    }
}

impl FileIo for Scheduler {
    async fn file_read_at(&self, file: Arc<File>, buffer: Vec<u8>, offset: u64) -> BufferResult {
        self.handle.file_read_at(file, buffer, offset).await
    }

    async fn file_write_at(&self, file: Arc<File>, buffer: Vec<u8>, offset: u64) -> BufferResult {
        self.handle.file_write_at(file, buffer, offset).await
    }
}

impl Clock for Scheduler {
    async fn sleep(&self, duration: Duration) {
        self.handle.sleep(duration).await;
    }
}
