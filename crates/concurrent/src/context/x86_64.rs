use super::{Context, CoroutineStart, aligned_stack_top, entry_address};
use crate::stack::Stack;
use std::ffi::c_void;
use std::io;
use std::ptr;

const SAVED_REGISTERS: usize = 6;

pub(super) fn initialize(
    context: &mut Context,
    stack: &Stack,
    start: CoroutineStart,
) -> io::Result<()> {
    let mut entry = entry_address(start);

    #[cfg(coroutine_shadow_stack)]
    let shadow_stack_pointer = if shadow_stack_enabled() {
        let shadow_stack_size = stack.size().checked_add(7).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "shadow stack size overflow")
        })? & !7;
        let shadow_stack = allocate_shadow_stack(shadow_stack_size)?;
        context.shadow_stack = shadow_stack;
        context.shadow_stack_size = shadow_stack_size;

        let shadow_stack_top = unsafe { shadow_stack.add(shadow_stack_size) };
        // SAFETY: CRuby's amd64 assembly helper initializes the shadow-stack
        // restore token at the top of this newly allocated mapping.
        unsafe { initialize_shadow_stack(shadow_stack_top) }
    } else {
        ptr::null_mut()
    };

    #[cfg(coroutine_shadow_stack)]
    if !shadow_stack_pointer.is_null() {
        entry = start_trampoline_address();
    }

    // Match amd64/Context.h: a return address, the entry point, then six saved
    // registers. CET adds the saved shadow-stack pointer below those registers.
    unsafe {
        let mut stack_pointer = aligned_stack_top(stack);
        stack_pointer = stack_pointer.sub(1);
        stack_pointer.write(ptr::null_mut());
        stack_pointer = stack_pointer.sub(1);
        stack_pointer.write(entry);
        stack_pointer = stack_pointer.sub(SAVED_REGISTERS);
        stack_pointer.write_bytes(0, SAVED_REGISTERS);

        #[cfg(coroutine_shadow_stack)]
        if !shadow_stack_pointer.is_null() {
            // The trampoline jumps to the real entry point in r12.
            stack_pointer.add(3).write(entry_address(start));
            stack_pointer = stack_pointer.sub(1);
            stack_pointer.write(shadow_stack_pointer);
        }

        context.stack_pointer = stack_pointer;
    }

    Ok(())
}

#[cfg(coroutine_shadow_stack)]
const ARCH_SHSTK_STATUS: libc::c_ulong = 0x5005;
#[cfg(coroutine_shadow_stack)]
const ARCH_SHSTK_SHSTK: libc::c_ulong = 1;
#[cfg(coroutine_shadow_stack)]
const SHADOW_STACK_SET_TOKEN: libc::c_ulong = 1;
#[cfg(coroutine_shadow_stack)]
const SYS_MAP_SHADOW_STACK: libc::c_long = 453;

#[cfg(coroutine_shadow_stack)]
unsafe extern "C" {
    fn coroutine_initialize_shadow_stack(shadow_stack_pointer: *mut c_void) -> *mut c_void;
    fn coroutine_start_trampoline() -> !;
}

#[cfg(coroutine_shadow_stack)]
fn shadow_stack_enabled() -> bool {
    let mut features: libc::c_ulong = 0;
    // SAFETY: arch_prctl writes one unsigned long to the supplied pointer.
    unsafe {
        libc::syscall(
            libc::SYS_arch_prctl,
            ARCH_SHSTK_STATUS,
            &mut features as *mut libc::c_ulong,
        ) == 0
            && features & ARCH_SHSTK_SHSTK != 0
    }
}

#[cfg(coroutine_shadow_stack)]
fn allocate_shadow_stack(size: usize) -> io::Result<*mut c_void> {
    // SAFETY: map_shadow_stack accepts an address hint, a non-zero size, and
    // the flag requesting a restore token at the top of the mapping.
    let mapping =
        unsafe { libc::syscall(SYS_MAP_SHADOW_STACK, 0usize, size, SHADOW_STACK_SET_TOKEN) };

    if mapping == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(mapping as *mut c_void)
    }
}

#[cfg(coroutine_shadow_stack)]
unsafe fn initialize_shadow_stack(top: *mut c_void) -> *mut c_void {
    // SAFETY: top is the one-past-the-end address of a shadow stack mapping.
    unsafe { coroutine_initialize_shadow_stack(top) }
}

#[cfg(coroutine_shadow_stack)]
fn start_trampoline_address() -> *mut c_void {
    coroutine_start_trampoline as *const () as *mut c_void
}
