use crate::owner::Owner;
use crate::{SchedulerHandle, Spawn, SpawnError, Task, TaskHandle};
use std::future::Future;
use std::sync::Arc;

/// Explicit ownership of child tasks running on a scheduler.
///
/// Dropping a barrier closes it and requests cancellation of its children.
/// Use `stop().await` to wait for their synchronous destructors. `wait().await`
/// waits for normal completion. Task results are observed through join handles.
/// All children must own their captured data (`Send + 'static`).
pub struct Barrier {
    scheduler: SchedulerHandle,
    owner: Arc<Owner>,
}

impl Barrier {
    /// Create an empty child owner using this scheduler for execution.
    pub fn new(scheduler: SchedulerHandle) -> Self {
        Self {
            scheduler,
            owner: Arc::new(Owner::new()),
        }
    }

    /// Spawn a child owned by this barrier.
    pub fn spawn<FutureType>(
        &self,
        future: FutureType,
    ) -> Result<TaskHandle<FutureType::Output>, SpawnError>
    where
        FutureType: Future + Send + 'static,
        FutureType::Output: Send + 'static,
    {
        Spawn::spawn(self, future)
    }

    /// Prevent further children from being spawned. Existing children continue.
    pub fn close(&self) {
        self.scheduler.shared.close_owner(&self.owner, false);
    }

    /// Wait until no children remain. Call `close` first for a closed set.
    ///
    /// This does not collect task outputs or propagate task panics; await each
    /// join handle when its result matters. Cancelling this wait leaves children
    /// running until the barrier is stopped or dropped.
    ///
    /// # Panics
    /// Panics if called by a child belonging to this barrier (it cannot join itself).
    pub async fn wait(&self) {
        self.assert_not_child();
        self.owner.wait().await;
    }

    /// Close the barrier, request cancellation, and wait for children to stop.
    /// Dropping this future after it is polled leaves cancellation requested.
    /// An in-progress poll must return before its task can be destroyed.
    ///
    /// # Panics
    /// Panics if called by one of this barrier's children.
    pub async fn stop(&self) {
        self.assert_not_child();
        self.scheduler.shared.close_owner(&self.owner, true);
        self.owner.wait().await;
    }

    /// Whether all direct children have finished and been destroyed.
    pub fn is_empty(&self) -> bool {
        self.owner.is_empty()
    }

    fn assert_not_child(&self) {
        assert!(
            Task::current().is_none_or(|task| !Arc::ptr_eq(&task.state.owner, &self.owner)),
            "a task cannot wait for its own barrier"
        );
    }
}

impl Spawn for Barrier {
    type Handle<Output>
        = TaskHandle<Output>
    where
        Output: Send + 'static;

    fn spawn<FutureType>(
        &self,
        future: FutureType,
    ) -> Result<TaskHandle<FutureType::Output>, SpawnError>
    where
        FutureType: Future + Send + 'static,
        FutureType::Output: Send + 'static,
    {
        self.scheduler.shared.spawn(&self.owner, future)
    }
}

impl Drop for Barrier {
    fn drop(&mut self) {
        self.scheduler.shared.close_owner(&self.owner, true);
    }
}
