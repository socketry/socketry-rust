// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

use crate::TaskError;
use event_listener::Event;
use std::fmt;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// The common spawning contract for a scheduler and an explicit child owner.
pub trait Spawn {
    /// The runtime's awaitable task handle. Runtime adapters may use their own
    /// representation while preserving the ownership and result contract.
    type Handle<Output>: Future<Output = Result<Output, TaskError>> + Send
    where
        Output: Send + 'static;

    /// Register a task with this owner before making it runnable.
    ///
    /// Dropping the returned join handle does not cancel the owned task.
    /// If the owner is closed, the future is dropped without being polled.
    fn spawn<FutureType>(
        &self,
        future: FutureType,
    ) -> Result<Self::Handle<FutureType::Output>, SpawnError>
    where
        FutureType: Future + Send + 'static,
        FutureType::Output: Send + 'static;
}

/// A task could not be registered with its owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpawnError {
    /// The scheduler has begun shutting down.
    SchedulerClosed,
    /// The barrier has stopped accepting children.
    OwnerClosed,
    /// All task identifiers have been used.
    IdentifiersExhausted,
}

impl fmt::Display for SpawnError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::SchedulerClosed => "the scheduler is closed",
            Self::OwnerClosed => "the task owner is closed",
            Self::IdentifiersExhausted => "task identifiers are exhausted",
        })
    }
}

impl std::error::Error for SpawnError {}

pub(crate) struct Owner {
    // Admission and closing are serialized by Shared::registry.
    pub(crate) closed: AtomicBool,
    pub(crate) remaining: AtomicUsize,
    pub(crate) completion: Event,
}

impl Owner {
    pub(crate) fn new() -> Self {
        Self {
            closed: AtomicBool::new(false),
            remaining: AtomicUsize::new(0),
            completion: Event::new(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.remaining.load(Ordering::Acquire) == 0
    }

    pub(crate) async fn wait(&self) {
        loop {
            // Register before checking the condition to retain racing completions.
            let listener = self.completion.listen();
            if self.is_empty() {
                return;
            }
            listener.await;
        }
    }
}

#[cfg(test)]
mod tests;
