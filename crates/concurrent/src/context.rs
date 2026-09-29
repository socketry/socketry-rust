use crate::stack::Stack;
use std::ffi::c_void;
use std::io;

#[cfg(coroutine_address_sanitizer)]
unsafe extern "C" {
    #[link_name = "__sanitizer_start_switch_fiber"]
    fn address_sanitizer_start_switch_fiber(
        saved_fake_stack: *mut *mut c_void,
        destination_stack_bottom: *const c_void,
        destination_stack_size: usize,
    );
    #[link_name = "__sanitizer_finish_switch_fiber"]
    fn address_sanitizer_finish_switch_fiber(
        saved_fake_stack: *mut c_void,
        previous_stack_bottom: *mut *const c_void,
        previous_stack_size: *mut usize,
    );
}

#[cfg(coroutine_thread_sanitizer)]
unsafe extern "C" {
    #[link_name = "__tsan_get_current_fiber"]
    fn thread_sanitizer_current_fiber() -> *mut c_void;
    #[link_name = "__tsan_create_fiber"]
    fn thread_sanitizer_create_fiber(flags: u32) -> *mut c_void;
    #[link_name = "__tsan_destroy_fiber"]
    fn thread_sanitizer_destroy_fiber(fiber: *mut c_void);
    #[link_name = "__tsan_switch_to_fiber"]
    fn thread_sanitizer_switch_to_fiber(fiber: *mut c_void, flags: u32);
}

#[cfg(target_arch = "aarch64")]
#[path = "context/aarch64.rs"]
mod architecture;
#[cfg(target_arch = "arm")]
#[path = "context/arm.rs"]
mod architecture;
#[cfg(target_arch = "loongarch64")]
#[path = "context/loongarch64.rs"]
mod architecture;
#[cfg(all(target_arch = "powerpc", target_endian = "big"))]
#[path = "context/powerpc.rs"]
mod architecture;
#[cfg(all(target_arch = "powerpc64", target_endian = "big"))]
#[path = "context/powerpc64.rs"]
mod architecture;
#[cfg(all(target_arch = "powerpc64", target_endian = "little"))]
#[path = "context/powerpc64le.rs"]
mod architecture;
#[cfg(target_arch = "riscv64")]
#[path = "context/riscv64.rs"]
mod architecture;
#[cfg(target_arch = "x86")]
#[path = "context/x86.rs"]
mod architecture;
#[cfg(target_arch = "x86_64")]
#[path = "context/x86_64.rs"]
mod architecture;

#[cfg(not(any(
    target_arch = "aarch64",
    target_arch = "arm",
    target_arch = "loongarch64",
    all(target_arch = "powerpc", target_endian = "big"),
    target_arch = "powerpc64",
    target_arch = "riscv64",
    target_arch = "x86",
    target_arch = "x86_64",
)))]
compile_error!("socketry-concurrent has no context implementation for this architecture");

#[repr(C)]
pub(crate) struct Context {
    // The assembly context-switch routines load this field from offset zero.
    stack_pointer: *mut *mut c_void,
    #[allow(dead_code)]
    argument: *mut c_void,
    #[cfg(coroutine_address_sanitizer)]
    address_sanitizer_fake_stack: *mut c_void,
    #[cfg(coroutine_address_sanitizer)]
    address_sanitizer_stack_base: *const c_void,
    #[cfg(coroutine_address_sanitizer)]
    address_sanitizer_stack_size: usize,
    #[cfg(coroutine_thread_sanitizer)]
    thread_sanitizer_fiber: *mut c_void,
    #[cfg(coroutine_thread_sanitizer)]
    thread_sanitizer_fiber_owned: i32,
    #[cfg(coroutine_shadow_stack)]
    shadow_stack: *mut c_void,
    #[cfg(coroutine_shadow_stack)]
    shadow_stack_size: usize,
}

const _: () = assert!(std::mem::offset_of!(Context, stack_pointer) == 0);
const _: () =
    assert!(std::mem::offset_of!(Context, argument) == std::mem::size_of::<*mut c_void>());

#[cfg(coroutine_address_sanitizer)]
const _: () = assert!(
    std::mem::offset_of!(Context, address_sanitizer_fake_stack)
        == 2 * std::mem::size_of::<*mut c_void>()
);
#[cfg(coroutine_address_sanitizer)]
const _: () = assert!(
    std::mem::offset_of!(Context, address_sanitizer_stack_base)
        == 3 * std::mem::size_of::<*mut c_void>()
);
#[cfg(coroutine_address_sanitizer)]
const _: () = assert!(
    std::mem::offset_of!(Context, address_sanitizer_stack_size)
        == 4 * std::mem::size_of::<*mut c_void>()
);

#[cfg(coroutine_thread_sanitizer)]
const _: () = assert!(
    std::mem::offset_of!(Context, thread_sanitizer_fiber) == {
        2 * std::mem::size_of::<*mut c_void>()
            + if cfg!(coroutine_address_sanitizer) {
                3 * std::mem::size_of::<*mut c_void>()
            } else {
                0
            }
    }
);
#[cfg(coroutine_thread_sanitizer)]
const _: () = assert!(
    std::mem::offset_of!(Context, thread_sanitizer_fiber_owned)
        == std::mem::offset_of!(Context, thread_sanitizer_fiber)
            + std::mem::size_of::<*mut c_void>()
);

#[cfg(coroutine_shadow_stack)]
const _: () = assert!(
    std::mem::offset_of!(Context, shadow_stack) == {
        2 * std::mem::size_of::<*mut c_void>()
            + if cfg!(coroutine_address_sanitizer) {
                3 * std::mem::size_of::<*mut c_void>()
            } else {
                0
            }
            + if cfg!(coroutine_thread_sanitizer) {
                2 * std::mem::size_of::<*mut c_void>()
            } else {
                0
            }
    }
);
#[cfg(coroutine_shadow_stack)]
const _: () = assert!(
    std::mem::offset_of!(Context, shadow_stack_size)
        == std::mem::offset_of!(Context, shadow_stack) + std::mem::size_of::<*mut c_void>()
);

#[cfg(target_arch = "x86")]
pub(crate) type CoroutineStart = extern "fastcall" fn(*mut Context, *mut Context) -> !;

#[cfg(not(target_arch = "x86"))]
pub(crate) type CoroutineStart = extern "C" fn(*mut Context, *mut Context) -> !;

#[cfg(target_arch = "x86")]
unsafe extern "fastcall" {
    #[link_name = "coroutine_transfer"]
    fn transfer(current: *mut Context, target: *mut Context) -> *mut Context;
}

#[cfg(not(target_arch = "x86"))]
unsafe extern "C" {
    #[link_name = "coroutine_transfer"]
    fn transfer(current: *mut Context, target: *mut Context) -> *mut Context;
}

impl Context {
    fn uninitialized() -> Self {
        Self {
            stack_pointer: std::ptr::null_mut(),
            argument: std::ptr::null_mut(),
            #[cfg(coroutine_address_sanitizer)]
            address_sanitizer_fake_stack: std::ptr::null_mut(),
            #[cfg(coroutine_address_sanitizer)]
            address_sanitizer_stack_base: std::ptr::null(),
            #[cfg(coroutine_address_sanitizer)]
            address_sanitizer_stack_size: 0,
            #[cfg(coroutine_thread_sanitizer)]
            thread_sanitizer_fiber: std::ptr::null_mut(),
            #[cfg(coroutine_thread_sanitizer)]
            thread_sanitizer_fiber_owned: 0,
            #[cfg(coroutine_shadow_stack)]
            shadow_stack: std::ptr::null_mut(),
            #[cfg(coroutine_shadow_stack)]
            shadow_stack_size: 0,
        }
    }

    pub(crate) fn empty() -> Self {
        #[allow(unused_mut)]
        let mut context = Self::uninitialized();
        #[cfg(coroutine_thread_sanitizer)]
        {
            // SAFETY: this borrows the current operating system thread's
            // ThreadSanitizer fiber context;
            // its lifetime is managed by the sanitizer runtime.
            context.thread_sanitizer_fiber = unsafe { thread_sanitizer_current_fiber() };
        }
        context
    }

    pub(crate) fn for_stack(stack: &Stack, start: CoroutineStart) -> io::Result<Self> {
        let mut context = Self::uninitialized();
        #[cfg(coroutine_address_sanitizer)]
        {
            context.address_sanitizer_stack_base = stack.base().cast();
            context.address_sanitizer_stack_size = stack.size();
        }
        #[cfg(coroutine_thread_sanitizer)]
        {
            // SAFETY: ThreadSanitizer creates an independent logical-fiber
            // context for this stack; Context::drop releases it after use.
            context.thread_sanitizer_fiber = unsafe { thread_sanitizer_create_fiber(0) };
            context.thread_sanitizer_fiber_owned = 1;
        }
        architecture::initialize(&mut context, stack, start)?;
        Ok(context)
    }

    pub(crate) unsafe fn switch(current: *mut Context, target: *mut Context) {
        unsafe { Self::switch_inner(current, target, false) }
    }

    pub(crate) unsafe fn switch_final(current: *mut Context, target: *mut Context) {
        unsafe { Self::switch_inner(current, target, true) }
    }

    unsafe fn switch_inner(current: *mut Context, target: *mut Context, final_switch: bool) {
        #[cfg(coroutine_address_sanitizer)]
        unsafe {
            let saved_fake_stack = if final_switch {
                // AddressSanitizer destroys this fake stack when a fiber will
                // never resume.
                std::ptr::null_mut()
            } else {
                std::ptr::addr_of_mut!((*current).address_sanitizer_fake_stack)
            };
            address_sanitizer_start_switch_fiber(
                saved_fake_stack,
                (*target).address_sanitizer_stack_base,
                (*target).address_sanitizer_stack_size,
            );
        }

        #[cfg(not(coroutine_address_sanitizer))]
        let _ = final_switch;

        #[cfg(coroutine_thread_sanitizer)]
        unsafe {
            // Match CRuby's transfer hook: ThreadSanitizer tracks the logical
            // fiber separately from the operating system thread's native stack.
            thread_sanitizer_switch_to_fiber((*target).thread_sanitizer_fiber, 0);
        }

        // SAFETY: both pointers refer to initialized contexts that remain alive
        // on the current thread for the duration of the switch.
        unsafe {
            let _ = transfer(current, target);
        }

        #[cfg(coroutine_address_sanitizer)]
        unsafe {
            address_sanitizer_finish_switch_fiber(
                (*current).address_sanitizer_fake_stack,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        }
    }

    pub(crate) unsafe fn finish_initial_switch(from: *mut Context, target: *mut Context) {
        #[cfg(coroutine_address_sanitizer)]
        unsafe {
            address_sanitizer_finish_switch_fiber(
                (*target).address_sanitizer_fake_stack,
                std::ptr::addr_of_mut!((*from).address_sanitizer_stack_base),
                std::ptr::addr_of_mut!((*from).address_sanitizer_stack_size),
            );
        }

        #[cfg(not(coroutine_address_sanitizer))]
        let _ = (from, target);
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        #[cfg(coroutine_thread_sanitizer)]
        if self.thread_sanitizer_fiber_owned != 0 && !self.thread_sanitizer_fiber.is_null() {
            // SAFETY: this context owns the fiber handle and is no longer
            // running when its containing task coroutine is dropped.
            unsafe {
                thread_sanitizer_destroy_fiber(self.thread_sanitizer_fiber);
            }
        }

        #[cfg(coroutine_shadow_stack)]
        if !self.shadow_stack.is_null() {
            // SAFETY: this mapping was allocated for this context and has not
            // been released before this drop.
            unsafe {
                libc::munmap(self.shadow_stack, self.shadow_stack_size);
            }
        }
    }
}

pub(super) fn aligned_stack_top(stack: &Stack) -> *mut *mut c_void {
    // SAFETY: Stack owns a mapping with at least `size` writable bytes from
    // `base`, followed by a guard page.
    let top = unsafe { stack.base().add(stack.size()) } as usize;
    (top & !0xF) as *mut *mut c_void
}

pub(super) fn entry_address(start: CoroutineStart) -> *mut c_void {
    start as *const () as *mut c_void
}
