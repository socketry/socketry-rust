use crate::stack::Stack;
use std::io;
use std::ptr;

#[repr(C)]
pub(crate) struct Context {
    stack_pointer: *mut usize,
}

unsafe extern "C" {
    #[link_name = "coroutine_transfer"]
    fn transfer(current: *mut Context, target: *mut Context) -> *mut Context;
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
unsafe extern "C" {
    #[link_name = "coroutine_initialize_shadow_stack"]
    fn initialize_shadow_stack(pointer: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    #[link_name = "coroutine_start_trampoline"]
    fn start_trampoline() -> !;
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
pub(crate) struct ShadowStack {
    mapping: *mut std::ffi::c_void,
    size: usize,
    pointer: *mut std::ffi::c_void,
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
impl ShadowStack {
    pub(crate) fn allocate(stack_size: usize) -> io::Result<Option<Self>> {
        const ARCH_SHSTK_STATUS: libc::c_ulong = 0x5005;
        const ARCH_SHSTK_SHSTK: libc::c_ulong = 1;
        const SHADOW_STACK_SET_TOKEN: libc::c_ulong = 1;
        const SYS_MAP_SHADOW_STACK: libc::c_long = 453;

        let mut features: libc::c_ulong = 0;
        // SAFETY: arch_prctl writes the status word to the supplied valid pointer.
        let status = unsafe {
            libc::syscall(
                libc::SYS_arch_prctl,
                ARCH_SHSTK_STATUS,
                &mut features as *mut libc::c_ulong,
            )
        };
        if status != 0 || features & ARCH_SHSTK_SHSTK == 0 {
            return Ok(None);
        }

        // SAFETY: sysconf has no pointer arguments and returns the OS page size.
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if page_size <= 0 {
            return Err(io::Error::last_os_error());
        }
        let page_size = page_size as usize;
        let size = stack_size
            .checked_add(page_size - 1)
            .map(|n| n / page_size * page_size)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "shadow stack size overflow")
            })?;

        // SAFETY: Linux map_shadow_stack creates an owned shadow-stack mapping.
        let mapping = unsafe {
            libc::syscall(
                SYS_MAP_SHADOW_STACK,
                ptr::null_mut::<std::ffi::c_void>(),
                size,
                SHADOW_STACK_SET_TOKEN,
            ) as *mut std::ffi::c_void
        };
        if mapping as isize == -1 {
            return Err(io::Error::last_os_error());
        }

        // SAFETY: the mapping is size bytes and ends with the kernel restore token.
        let end = unsafe { mapping.cast::<u8>().add(size).cast() };
        // SAFETY: CRuby's routine initializes the new shadow stack then restores this one.
        let pointer = unsafe { initialize_shadow_stack(end) };

        Ok(Some(Self {
            mapping,
            size,
            pointer,
        }))
    }

    pub(crate) fn pointer(&self) -> *mut std::ffi::c_void {
        self.pointer
    }
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
impl Drop for ShadowStack {
    fn drop(&mut self) {
        // SAFETY: this mapping is owned by this ShadowStack.
        unsafe { libc::munmap(self.mapping, self.size) };
    }
}

impl Context {
    pub(crate) const fn empty() -> Self {
        Self {
            stack_pointer: ptr::null_mut(),
        }
    }

    pub(crate) fn initialize(
        &mut self,
        stack: &Stack,
        entry: extern "C" fn() -> !,
        shadow_stack_pointer: *mut std::ffi::c_void,
    ) {
        let top = stack.top() as usize & !0xf;

        #[cfg(target_arch = "aarch64")]
        {
            // CRuby's AArch64 context saves d8-d15 and x19-x30 (160 bytes).
            let frame = (top - 160) as *mut usize;
            let entry = sign_instruction_address(entry as usize, top);
            // SAFETY: this frame is inside the usable stack and is ABI aligned.
            unsafe {
                ptr::write_bytes(frame, 0, 20);
                frame.add(19).write(entry);
            }
            self.stack_pointer = frame;
        }

        #[cfg(target_arch = "x86_64")]
        {
            #[cfg(target_os = "linux")]
            let first_entry = if shadow_stack_pointer.is_null() {
                entry as usize
            } else {
                start_trampoline as usize
            };

            #[cfg(not(target_os = "linux"))]
            let first_entry = entry as usize;

            let mut cursor = top as *mut usize;
            // The first RET reaches the fiber entry; the zero word is its return sentinel.
            unsafe {
                cursor = cursor.sub(1);
                cursor.write(0);
                cursor = cursor.sub(1);
                cursor.write(first_entry);
                cursor = cursor.sub(6);
                ptr::write_bytes(cursor, 0, 6);
            }

            #[cfg(target_os = "linux")]
            {
                if !shadow_stack_pointer.is_null() {
                    // CRuby's trampoline jumps to the entry address in the saved r12 slot.
                    unsafe { cursor.add(3).write(entry as usize) };
                }
                // CRuby's transfer routine consumes this slot only when CET is active.
                cursor = unsafe { cursor.sub(1) };
                // SAFETY: cursor remains inside the usable stack.
                unsafe { cursor.write(shadow_stack_pointer as usize) };
            }

            self.stack_pointer = cursor;
        }
    }

    pub(crate) unsafe fn switch(current: *mut Context, target: *mut Context) {
        // SAFETY: both contexts have valid stack frames and remain on their owner thread.
        unsafe {
            transfer(current, target);
        }
    }
}

#[cfg(target_arch = "aarch64")]
fn sign_instruction_address(address: usize, modifier: usize) -> usize {
    let mut signed = address;
    // The instruction is a NOP on targets without pointer authentication and
    // matches CRuby's PACIA1716 setup when PAC is enabled.
    unsafe {
        core::arch::asm!(
            "hint #8",
            inout("x17") signed,
            in("x16") modifier,
            options(nostack),
        );
    }
    signed
}
