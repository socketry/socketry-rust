use crate::context::{Context, CoroutineStart};
use crate::scheduler::{SchedulerHandle, TaskHandle};
use crate::stack::Stack;
use std::any::Any;
use std::cell::{Cell, UnsafeCell};
use std::io;
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::{Rc, Weak};

type CoroutineCell = UnsafeCell<CoroutineInner>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    New,
    Running,
    Suspended,
    Finished,
}

struct CoroutineInner {
    self_reference: Weak<CoroutineCell>,
    context: Context,
    scheduler: Option<SchedulerHandle>,
    task: Option<TaskHandle>,
    stack: Option<Stack>,
    entry: Option<Box<dyn FnOnce() + 'static>>,
    panic: Option<Box<dyn Any + Send + 'static>>,
    cancel_requested: bool,
    state: State,
}

#[derive(Debug)]
struct Cancelled;

thread_local! {
    static ACTIVE_TASK: Cell<*mut CoroutineInner> = const { Cell::new(std::ptr::null_mut()) };
    static ACTIVE_SCHEDULER_CONTEXT: Cell<*mut Context> = const { Cell::new(std::ptr::null_mut()) };
}

/// A scheduler-owned stackful task implementation.
pub(crate) struct Coroutine {
    inner: Rc<CoroutineCell>,
    _thread_affine: PhantomData<Rc<()>>,
}

/// A non-owning, thread-affine reference to a scheduler-owned coroutine.
#[derive(Clone)]
pub(crate) struct CoroutineHandle {
    inner: Weak<CoroutineCell>,
    _thread_affine: PhantomData<Rc<()>>,
}

impl Coroutine {
    /// Return a non-owning handle to the currently running task, if there is one.
    ///
    /// This returns `None` on an ordinary operating-system thread stack outside
    /// a task. The returned handle does not keep the task alive.
    #[cfg(test)]
    pub(crate) fn current() -> Option<CoroutineHandle> {
        let inner = ACTIVE_TASK.with(Cell::get);
        if inner.is_null() {
            return None;
        }

        // SAFETY: the scheduler owns the active task's control block while the
        // task is running.
        let current = unsafe { (*inner).self_reference.upgrade() }?;
        Some(CoroutineHandle {
            inner: Rc::downgrade(&current),
            _thread_affine: PhantomData,
        })
    }

    /// Allocate a guarded stack and create a task coroutine around the closure.
    #[cfg(test)]
    pub(crate) fn new(stack_size: usize, entry: impl FnOnce() + 'static) -> io::Result<Self> {
        Self::with_stack(Stack::new(stack_size)?, entry)
    }

    /// Create a task coroutine using an existing guarded stack.
    pub(crate) fn with_stack(stack: Stack, entry: impl FnOnce() + 'static) -> io::Result<Self> {
        let context = Context::for_stack(&stack, coroutine_entry as CoroutineStart)?;
        let inner = Rc::new_cyclic(|self_reference| {
            UnsafeCell::new(CoroutineInner {
                self_reference: self_reference.clone(),
                context,
                scheduler: None,
                task: None,
                stack: Some(stack),
                entry: Some(Box::new(entry)),
                panic: None,
                cancel_requested: false,
                state: State::New,
            })
        });

        Ok(Self {
            inner,
            _thread_affine: PhantomData,
        })
    }

    /// Return a non-owning handle that can be captured by another task.
    #[cfg(test)]
    pub(crate) fn handle(&self) -> CoroutineHandle {
        CoroutineHandle {
            inner: Rc::downgrade(&self.inner),
            _thread_affine: PhantomData,
        }
    }

    pub(crate) fn set_scheduler(&mut self, scheduler: SchedulerHandle, task: TaskHandle) {
        // SAFETY: a task receives its scheduler before it is started, and the
        // mutable borrow prevents concurrent access to its control block.
        unsafe {
            let inner = &mut *self.inner.get();
            assert_eq!(
                inner.state,
                State::New,
                "cannot change a running task's scheduler"
            );
            inner.scheduler = Some(scheduler);
            inner.task = Some(task);
        }
    }

    /// Dispatch this task from its scheduler context until it parks or finishes.
    pub(crate) fn dispatch(&mut self, scheduler_context: &mut Context) {
        dispatch_cell(&self.inner, scheduler_context);
    }

    /// Return whether the entry closure has returned or panicked.
    pub(crate) fn is_finished(&self) -> bool {
        // SAFETY: tasks are thread-affine, and observing this field does not
        // overlap a mutable access on another OS thread.
        unsafe { (*self.inner.get()).state == State::Finished }
    }

    /// Cancel this task and unwind its suspended stack.
    ///
    /// Cancellation resumes the task at its current park or transfer point
    /// and runs destructors on the task stack.
    pub(crate) fn cancel(&mut self, scheduler_context: &mut Context) {
        cancel_cell(&self.inner, scheduler_context);
    }

    /// Transfer execution to another scheduler-owned task.
    ///
    /// The target continues from its last suspension point. The source remains
    /// suspended at this call until another task transfers to it or the
    /// scheduler runs it again. The target's completion returns to the active
    /// scheduler context.
    pub(crate) fn transfer(target: &CoroutineHandle) {
        let source_pointer = ACTIVE_TASK.with(Cell::get);
        assert!(
            !source_pointer.is_null(),
            "task transfer called outside a task"
        );

        // SAFETY: ACTIVE_TASK points at the currently running task. The
        // scheduler owns its control block for the duration of execution.
        let _source = unsafe {
            (*source_pointer)
                .self_reference
                .upgrade()
                .expect("active task is no longer owned")
        };
        let destination = target
            .inner
            .upgrade()
            .expect("cannot transfer to a task that has been dropped");
        let destination_pointer = destination.get();
        assert_ne!(
            source_pointer, destination_pointer,
            "a task cannot transfer to itself"
        );

        // SAFETY: the scheduler owns every task control block until it is
        // finished, and transfers stay on the scheduler's owning thread.
        unsafe {
            let source_inner = &mut *source_pointer;
            let destination_inner = &mut *destination_pointer;
            assert_eq!(
                source_inner.state,
                State::Running,
                "source task is not running"
            );
            assert_ne!(
                destination_inner.state,
                State::Running,
                "cannot transfer to a task that is already running"
            );
            assert_ne!(
                destination_inner.state,
                State::Finished,
                "cannot transfer to a finished task"
            );
            destination_inner.state = State::Running;
            source_inner.state = State::Suspended;
        }

        // `transfer` changes the active task identity. When this call resumes,
        // execution is back on the source task's stack.
        ACTIVE_TASK.with(|active| active.set(destination_pointer));
        // SAFETY: the scheduler retains both task controls for the full switch.
        // No references into either CoroutineInner are held across the switch.
        unsafe {
            Context::switch(
                std::ptr::addr_of_mut!((*source_pointer).context),
                std::ptr::addr_of_mut!((*destination_pointer).context),
            );
        }
        ACTIVE_TASK.with(|active| active.set(source_pointer));
        unsafe { (*source_pointer).state = State::Running };

        // A cancellation requested while this source was suspended is observed
        // immediately when its transfer call resumes.
        if unsafe { (*source_pointer).cancel_requested } {
            resume_unwind(Box::new(Cancelled));
        }
    }

    /// Park the currently running task at the scheduler context active for
    /// this dispatch.
    ///
    /// Calling this outside a task with an active scheduler context panics.
    pub(crate) fn park() {
        let inner = ACTIVE_TASK.with(Cell::get);
        assert!(!inner.is_null(), "cannot park outside a running task");

        // SAFETY: ACTIVE_TASK identifies the only currently running task.
        unsafe {
            let scheduler_context = ACTIVE_SCHEDULER_CONTEXT.with(Cell::get);
            assert!(
                !scheduler_context.is_null(),
                "cannot park without an active scheduler context"
            );
            (*inner).state = State::Suspended;
            Context::switch(std::ptr::addr_of_mut!((*inner).context), scheduler_context);
            (*inner).state = State::Running;
            if (*inner).cancel_requested {
                resume_unwind(Box::new(Cancelled));
            }
        }
    }

    pub(crate) fn take_stack(&mut self) -> Option<Stack> {
        debug_assert!(self.is_finished());
        // SAFETY: `&mut self` ensures there is no concurrent access to this
        // thread-affine task, and finished tasks are no longer running.
        unsafe { (&mut *self.inner.get()).stack.take() }
    }

    pub(crate) fn take_panic(&mut self) -> Option<Box<dyn Any + Send + 'static>> {
        // SAFETY: callers only take a panic after the coroutine has finished.
        unsafe { (&mut *self.inner.get()).panic.take() }
    }
}

pub(crate) fn current_scheduler_handle() -> Option<SchedulerHandle> {
    let inner = ACTIVE_TASK.with(Cell::get);
    if inner.is_null() {
        return None;
    }

    // SAFETY: ACTIVE_TASK points at the currently running task, whose
    // scheduler association remains stable for its lifetime.
    unsafe { (*inner).scheduler.clone() }
}

pub(crate) fn current_task_handle() -> Option<TaskHandle> {
    let inner = ACTIVE_TASK.with(Cell::get);
    if inner.is_null() {
        return None;
    }

    // SAFETY: the scheduler owns the active control block, and its task
    // association remains stable until the task has completed.
    unsafe { (*inner).task.clone() }
}

pub(crate) fn current_task_context() -> Option<(CoroutineHandle, SchedulerHandle, TaskHandle)> {
    let inner = ACTIVE_TASK.with(Cell::get);
    if inner.is_null() {
        return None;
    }

    // SAFETY: ACTIVE_TASK points at the currently running task, whose task
    // and scheduler associations remain stable for its lifetime.
    unsafe {
        let task = (*inner).task.clone()?;
        let scheduler = (*inner).scheduler.clone()?;
        let owner = (*inner).self_reference.upgrade()?;
        Some((
            CoroutineHandle {
                inner: Rc::downgrade(&owner),
                _thread_affine: PhantomData,
            },
            scheduler,
            task,
        ))
    }
}

impl CoroutineHandle {
    /// Return whether the referenced task has finished, or its owner is gone.
    #[cfg(test)]
    pub fn is_finished(&self) -> bool {
        self.inner.upgrade().is_none_or(|inner| {
            // SAFETY: handles are thread-affine and only inspect state.
            unsafe { (*inner.get()).state == State::Finished }
        })
    }

    pub(crate) fn is_current(&self) -> bool {
        let Some(inner) = self.inner.upgrade() else {
            return false;
        };

        ACTIVE_TASK.with(|active| active.get() == inner.get())
    }
}

#[cfg(test)]
impl std::fmt::Debug for CoroutineHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CoroutineHandle")
            .field("is_finished", &self.is_finished())
            .finish()
    }
}

fn dispatch_cell(cell: &Rc<CoroutineCell>, scheduler_context: &mut Context) {
    let inner = cell.get();
    // SAFETY: CoroutineCell is thread-affine, and the scheduler is the only
    // active stack when it dispatches a task.
    unsafe {
        if (*inner).state == State::Finished {
            return;
        }
        assert_ne!(
            (*inner).state,
            State::Running,
            "cannot dispatch a running task"
        );
        (*inner).state = State::Running;
    }

    let previous_scheduler_context = ACTIVE_SCHEDULER_CONTEXT
        .with(|active| active.replace(std::ptr::from_mut(scheduler_context)));
    ACTIVE_TASK.with(|active| {
        let previous_task = active.replace(inner);
        // SAFETY: both contexts are stable for the duration of this dispatch.
        // The assembly saves the scheduler stack before switching.
        unsafe {
            Context::switch(scheduler_context, std::ptr::addr_of_mut!((*inner).context));
        }
        active.set(previous_task);
    });
    ACTIVE_SCHEDULER_CONTEXT.with(|active| active.set(previous_scheduler_context));

    // SAFETY: the task is suspended or finished before control reaches here.
    let panic = unsafe { (*inner).panic.take() };
    if let Some(payload) = panic {
        resume_unwind(payload);
    }
}

fn cancel_cell(cell: &Rc<CoroutineCell>, scheduler_context: &mut Context) {
    let inner = cell.get();
    // SAFETY: CoroutineCell is thread-affine. Cancellation is requested from a
    // scheduler stack.
    unsafe {
        if (*inner).state == State::Finished {
            return;
        }
        if (*inner).state == State::New {
            (*inner).entry.take();
            (*inner).state = State::Finished;
            if let Some(task) = &(*inner).task {
                task.mark_completed();
            }
            return;
        }
        (*inner).cancel_requested = true;
    }

    while unsafe { (*inner).state != State::Finished } {
        dispatch_cell(cell, scheduler_context);
    }
}

#[cfg(target_arch = "x86")]
extern "fastcall" fn coroutine_entry(from: *mut Context, this: *mut Context) -> ! {
    // SAFETY: the assembly trampoline enters here on `this`'s initialized
    // stack, with valid context pointers supplied by Context::switch.
    unsafe { Context::finish_initial_switch(from, this) };
    coroutine_entry_inner()
}

#[cfg(not(target_arch = "x86"))]
extern "C" fn coroutine_entry(from: *mut Context, this: *mut Context) -> ! {
    // SAFETY: the assembly trampoline enters here on `this`'s initialized
    // stack, with valid context pointers supplied by Context::switch.
    unsafe { Context::finish_initial_switch(from, this) };
    coroutine_entry_inner()
}

fn coroutine_entry_inner() -> ! {
    let inner = ACTIVE_TASK.with(Cell::get);
    if inner.is_null() {
        std::process::abort();
    }

    // Do not hold a Rust reference to CoroutineInner while invoking user code:
    // the closure may park or transfer and let another task inspect this one.
    let entry = unsafe { (*inner).entry.take() };
    let result = match entry {
        Some(entry) => catch_unwind(AssertUnwindSafe(entry)),
        None => Ok(()),
    };

    let panic = match result {
        Err(payload) if payload.is::<Cancelled>() => None,
        Err(payload) => Some(payload),
        Ok(()) => None,
    };
    let scheduler_context = ACTIVE_SCHEDULER_CONTEXT.with(Cell::get);
    if scheduler_context.is_null() {
        std::process::abort();
    }

    // SAFETY: the trampoline caught the unwind and is abandoning this stack.
    // The scheduler owns every task context until the switch completes.
    unsafe {
        (*inner).panic = panic;
        (*inner).state = State::Finished;
        if let Some(task) = &(*inner).task {
            task.mark_completed();
        }
        Context::switch_final(std::ptr::addr_of_mut!((*inner).context), scheduler_context);
    }

    std::process::abort();
}
