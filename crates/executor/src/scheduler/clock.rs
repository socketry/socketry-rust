// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

use std::future::Future;
use std::time::Duration;

/// A runtime's monotonic sleep facility.
pub trait Clock: Send + Sync {
    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> + Send;
}
