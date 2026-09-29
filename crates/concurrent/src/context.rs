use crate::stack::Stack;
use std::ffi::c_void;

// The largest context supported by the vendored CRuby headers is currently
// smaller than this storage, including their optional sanitizer and CET
// fields. The C shim checks this size and alignment when it is compiled.
const CONTEXT_STORAGE_WORDS: usize = 16;

#[repr(C, align(16))]
pub(crate) struct Context {
    storage: [usize; CONTEXT_STORAGE_WORDS],
}

#[cfg(target_arch = "x86")]
pub(crate) type CoroutineStart = extern "fastcall" fn(*mut c_void, *mut c_void) -> !;

#[cfg(not(target_arch = "x86"))]
pub(crate) type CoroutineStart = extern "C" fn(*mut c_void, *mut c_void) -> !;

#[cfg(target_arch = "x86")]
unsafe extern "fastcall" {
    #[link_name = "coroutine_transfer"]
    fn transfer(current: *mut c_void, target: *mut c_void) -> *mut c_void;
}

#[cfg(not(target_arch = "x86"))]
unsafe extern "C" {
    #[link_name = "coroutine_transfer"]
    fn transfer(current: *mut c_void, target: *mut c_void) -> *mut c_void;
}

unsafe extern "C" {
    fn socketry_coroutine_context_initialize_main(context: *mut c_void);
    fn socketry_coroutine_context_initialize(
        context: *mut c_void,
        start: CoroutineStart,
        stack: *mut c_void,
        size: usize,
    );
    fn socketry_coroutine_context_destroy(context: *mut c_void);
}

impl Context {
    fn uninitialized() -> Self {
        Self {
            storage: [0; CONTEXT_STORAGE_WORDS],
        }
    }

    pub(crate) fn empty() -> Self {
        let mut context = Self::uninitialized();
        // SAFETY: storage is aligned and large enough for the vendored context
        // structure, as checked by context_shim.c.
        unsafe {
            socketry_coroutine_context_initialize_main(context.as_opaque());
        }
        context
    }

    pub(crate) fn for_stack(stack: &Stack, start: CoroutineStart) -> Self {
        let mut context = Self::uninitialized();
        // SAFETY: storage is aligned and large enough for the vendored context
        // structure. Stack owns a writable mapping of the supplied size.
        unsafe {
            socketry_coroutine_context_initialize(
                context.as_opaque(),
                start,
                stack.base().cast(),
                stack.size(),
            );
        }
        context
    }

    fn as_opaque(&mut self) -> *mut c_void {
        (self as *mut Self).cast()
    }

    pub(crate) unsafe fn switch(current: *mut Context, target: *mut Context) {
        // SAFETY: both pointers refer to initialized contexts that remain alive
        // on the current thread for the duration of the switch.
        unsafe {
            let _ = transfer(current.cast(), target.cast());
        }
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // SAFETY: this storage was initialized by the matching C helper and is
        // still alive. The helper releases any per-context native resources.
        unsafe {
            socketry_coroutine_context_destroy(self.as_opaque());
        }
    }
}
