// Released under the MIT License.
// Copyright, 2026, by Samuel Williams.

/// A socket readiness condition. Readiness can be spurious; retry nonblocking
/// operations and wait again when they return `WouldBlock`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Interest {
    Readable,
    Writable,
}
