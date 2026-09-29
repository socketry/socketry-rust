The complete `coroutine/` source tree, including platform backends and tests,
is copied verbatim from ruby/ruby at commit c3a285fcf2722ecb0808ad3b24b0f7b6a8b7b95f.

Upstream: https://github.com/ruby/ruby/tree/c3a285fcf2722ecb0808ad3b24b0f7b6a8b7b95f/coroutine

This crate currently integrates the `Context.S` and `Context.h` backends for
x86-64, x86, AArch64, 32-bit ARM, RISC-V64, LoongArch64, and applicable PowerPC
targets. The C shim in `src/context_shim.c` exposes the selected header's inline
initialization helpers to Rust. All other upstream variants remain available
in the vendored tree.
