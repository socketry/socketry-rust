use crate::context::{Context, CoroutineStart};
use crate::stack::Stack;
use std::any::Any;
use std::cell::Cell;
use std::io;
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    New,
    Running,
    Suspended,
    Finished,
}

struct FiberInner {
    context: Context,
    caller: Context,
    stack: Option<Stack>,
    entry: Option<Box<dyn FnOnce() + 'static>>,
    panic: Option<Box<dyn Any + Send + 'static>>,
    cancel_requested: bool,
    state: State,
}

#[derive(Debug)]
struct Cancelled;

thread_local! {
    static ACTIVE_FIBER: Cell<*mut FiberInner> = const { Cell::new(std::ptr::null_mut()) };
}

/// A stackful, cooperatively scheduled fiber.
///
/// Fibers are pinned to the OS thread on which they are created and are not
/// Send. Dropping a suspended fiber resumes it with a private cancellation
/// panic so Rust can unwind and drop its suspended frames.
pub struct Fiber {
    inner: Box<FiberInner>,
    _thread_affine: PhantomData<Rc<()>>,
}

impl Fiber {
    /// Allocate a guarded stack and create a fiber around the entry closure.
    pub fn new(stack_size: usize, entry: impl FnOnce() + 'static) -> io::Result<Self> {
        Self::with_stack(Stack::new(stack_size)?, entry)
    }

    /// Create a fiber using an existing guarded stack.
    pub fn with_stack(stack: Stack, entry: impl FnOnce() + 'static) -> io::Result<Self> {
        let context = Context::for_stack(&stack, fiber_entry as CoroutineStart);
        let caller = Context::empty();

        let inner = Box::new(FiberInner {
            context,
            caller,
            stack: Some(stack),
            entry: Some(Box::new(entry)),
            panic: None,
            cancel_requested: false,
            state: State::New,
        });

        Ok(Self {
            inner,
            _thread_affine: PhantomData,
        })
    }

    /// Resume execution until this fiber yields or completes.
    pub fn resume(&mut self) {
        let inner = self.inner.as_mut() as *mut FiberInner;
        // SAFETY: the pointer is stable because FiberInner is boxed. No reference
        // to its fields is kept across the context switch.
        unsafe {
            if (*inner).state == State::Finished {
                return;
            }
            (*inner).state = State::Running;
        }

        ACTIVE_FIBER.with(|active| {
            let previous = active.replace(inner);
            // SAFETY: these contexts belong to this fiber and it is resumed on its
            // owning thread. The assembly saves the current stack before switching.
            unsafe {
                Context::switch(
                    std::ptr::addr_of_mut!((*inner).caller),
                    std::ptr::addr_of_mut!((*inner).context),
                );
            }
            active.set(previous);
        });

        // SAFETY: the fiber is suspended or finished before control reaches here.
        let panic = unsafe { (*inner).panic.take() };
        if let Some(payload) = panic {
            resume_unwind(payload);
        }
    }

    /// Return whether the entry closure has returned or panicked.
    pub fn is_finished(&self) -> bool {
        self.inner.state == State::Finished
    }

    /// Cancel this fiber and unwind its suspended stack.
    ///
    /// Cancellation resumes the fiber at Fiber::yield_now and runs destructors
    /// on the fiber stack.
    pub fn cancel(&mut self) {
        if self.inner.state == State::Finished {
            return;
        }

        if self.inner.state == State::New {
            self.inner.entry.take();
            self.inner.state = State::Finished;
            return;
        }

        self.inner.cancel_requested = true;
        while !self.is_finished() {
            self.resume();
        }
    }

    /// Yield from the currently running fiber to its caller.
    ///
    /// Calling this outside a running fiber panics.
    pub fn yield_now() {
        let inner = ACTIVE_FIBER.with(Cell::get);
        assert!(!inner.is_null(), "Fiber::yield_now called outside a fiber");

        // SAFETY: ACTIVE_FIBER is set only while this fiber is running. Raw
        // pointers avoid retaining Rust references while another stack runs.
        unsafe {
            (*inner).state = State::Suspended;
            Context::switch(
                std::ptr::addr_of_mut!((*inner).context),
                std::ptr::addr_of_mut!((*inner).caller),
            );
            if (*inner).cancel_requested {
                std::panic::panic_any(Cancelled);
            }
        }
    }

    pub(crate) fn take_stack(&mut self) -> Option<Stack> {
        debug_assert!(self.is_finished());
        self.inner.stack.take()
    }
}

#[cfg(target_arch = "x86")]
extern "fastcall" fn fiber_entry(_from: *mut std::ffi::c_void, _self: *mut std::ffi::c_void) -> ! {
    fiber_entry_inner()
}

#[cfg(not(target_arch = "x86"))]
extern "C" fn fiber_entry(_from: *mut std::ffi::c_void, _self: *mut std::ffi::c_void) -> ! {
    fiber_entry_inner()
}

fn fiber_entry_inner() -> ! {
    let inner = ACTIVE_FIBER.with(Cell::get);
    if inner.is_null() {
        std::process::abort();
    }

    // Do not hold a Rust reference to FiberInner while invoking user code:
    // the user closure may yield and let the caller inspect the fiber.
    let entry = unsafe { (*inner).entry.take() };
    let result = match entry {
        Some(entry) => catch_unwind(AssertUnwindSafe(entry)),
        None => Ok(()),
    };
    let panic = match result {
        Err(payload) if payload.is::<Cancelled>() => None,
        result => result.err(),
    };

    unsafe {
        (*inner).panic = panic;
        (*inner).state = State::Finished;
        Context::switch(
            std::ptr::addr_of_mut!((*inner).context),
            std::ptr::addr_of_mut!((*inner).caller),
        );
    }

    // A completed fiber is always switched back to its caller.
    std::process::abort();
}

impl Drop for Fiber {
    fn drop(&mut self) {
        self.cancel();
    }
}
