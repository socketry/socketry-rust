use super::{Context, CoroutineStart, aligned_stack_top, entry_address};
use crate::stack::Stack;
use std::io;

const SAVED_REGISTERS: usize = 24;
const START_ADDRESS_INDEX: usize = 19;

pub(super) fn initialize(
    context: &mut Context,
    stack: &Stack,
    start: CoroutineStart,
) -> io::Result<()> {
    // Match ppc/Context.h: the entry starts after the global-prologue
    // instruction that initializes the TOC register.
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
