use super::{Context, CoroutineStart, aligned_stack_top, entry_address};
use crate::stack::Stack;
use std::io;

const SAVED_REGISTERS: usize = 66;
const START_ADDRESS_INDEX: usize = 18;

pub(super) fn initialize(
    context: &mut Context,
    stack: &Stack,
    start: CoroutineStart,
) -> io::Result<()> {
    // Match ppc64le/Context.h: preserve general, floating-point, and vector
    // register slots, then skip the TOC-initializing global prologue.
    unsafe {
        let stack_pointer = aligned_stack_top(stack).sub(SAVED_REGISTERS);
        stack_pointer.write_bytes(0, SAVED_REGISTERS);
        stack_pointer
            .add(START_ADDRESS_INDEX)
            .write(entry_address(start).wrapping_add(8));
        context.stack_pointer = stack_pointer;
    }

    Ok(())
}
