#![allow(dead_code)]

// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::time::Duration;

pub const TIMEOUT: Duration = Duration::from_secs(10);

pub fn receive<Output>(receiver: &Receiver<Output>) -> Output {
    receiver
        .recv_timeout(TIMEOUT)
        .expect("worker did not make progress before timeout")
}

pub struct CountDrop(pub Arc<AtomicUsize>);

impl Drop for CountDrop {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
