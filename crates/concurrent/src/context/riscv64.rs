use super::{Context, CoroutineStart, aligned_stack_top, entry_address};
use crate::stack::Stack;
use std::io;

const SAVED_REGISTER_BYTES: usize = 0xd0;
const SAVED_REGISTERS: usize = SAVED_REGISTER_BYTES / std::mem::size_of::<usize>();
const START_ADDRESS_INDEX: usize = 0xc0 / std::mem::size_of::<usize>();

pub(super) fn initialize(
    context: &mut Context,
    stack: &Stack,
    start: CoroutineStart,
) -> io::Result<()> {
    // Match riscv64/Context.h: save twenty-six register words and place the
    // entry address in the saved return-address slot.
    unsafe {
        let stack_pointer = aligned_stack_top(stack).sub(SAVED_REGISTERS);
        stack_pointer.write_bytes(0, SAVED_REGISTERS);
        stack_pointer
            .add(START_ADDRESS_INDEX)
            .write(entry_address(start));
        context.stack_pointer = stack_pointer;
    }

    Ok(())
}
