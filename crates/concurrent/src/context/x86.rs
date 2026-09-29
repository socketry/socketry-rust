use super::{Context, CoroutineStart, aligned_stack_top, entry_address};
use crate::stack::Stack;
use std::io;
use std::ptr;

const SAVED_REGISTERS: usize = 4;

pub(super) fn initialize(
    context: &mut Context,
    stack: &Stack,
    start: CoroutineStart,
) -> io::Result<()> {
    // Match x86/Context.h: place a null return address below the entry point,
    // then reserve four saved registers.
    unsafe {
        let mut stack_pointer = aligned_stack_top(stack);
        stack_pointer = stack_pointer.sub(1);
        stack_pointer.write(ptr::null_mut());
        stack_pointer = stack_pointer.sub(1);
        stack_pointer.write(entry_address(start));
        stack_pointer = stack_pointer.sub(SAVED_REGISTERS);
        stack_pointer.write_bytes(0, SAVED_REGISTERS);
        context.stack_pointer = stack_pointer;
    }

    Ok(())
}
