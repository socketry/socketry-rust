use super::{Context, CoroutineStart, aligned_stack_top, entry_address};
use crate::stack::Stack;
use std::io;

const SAVED_REGISTER_BYTES: usize = 0xa0;
const SAVED_REGISTERS: usize = SAVED_REGISTER_BYTES / std::mem::size_of::<usize>();
const START_ADDRESS_INDEX: usize = 0x98 / std::mem::size_of::<usize>();

pub(super) fn initialize(
    context: &mut Context,
    stack: &Stack,
    start: CoroutineStart,
) -> io::Result<()> {
    // Match arm64/Context.h: the frame stores twenty register words and the
    // initial program counter occupies the saved x30 slot.
    unsafe {
        let stack_top = aligned_stack_top(stack);
        let stack_pointer = stack_top.sub(SAVED_REGISTERS);
        stack_pointer.write_bytes(0, SAVED_REGISTERS);
        stack_pointer
            .add(START_ADDRESS_INDEX)
            .write(sign_instruction_address(
                entry_address(start),
                stack_top.cast(),
            ));
        context.stack_pointer = stack_pointer;
    }

    Ok(())
}

fn sign_instruction_address(
    address: *mut std::ffi::c_void,
    modifier: *mut std::ffi::c_void,
) -> *mut std::ffi::c_void {
    #[cfg(target_feature = "paca")]
    {
        let mut address = address;
        // PACIA1716 signs the address in x17 using the modifier in x16. The
        // HINT encoding is accepted by assemblers without PAC mnemonics.
        unsafe {
            std::arch::asm!(
                "hint #8",
                inout("x17") address,
                in("x16") modifier,
                options(nostack, preserves_flags),
            );
        }
        address
    }

    #[cfg(not(target_feature = "paca"))]
    {
        let _ = modifier;
        address
    }
}
