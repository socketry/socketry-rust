The complete `coroutine/` source tree, including platform backends and tests,
is copied verbatim from ruby/ruby at commit c3a285fcf2722ecb0808ad3b24b0f7b6a8b7b95f.

Upstream: https://github.com/ruby/ruby/tree/c3a285fcf2722ecb0808ad3b24b0f7b6a8b7b95f/coroutine

This crate assembles the upstream `Context.S` backends for x86-64, x86, AArch64,
32-bit ARM, RISC-V64, LoongArch64, and applicable PowerPC targets. The matching
`Context.h` stack initialization logic is ported to Rust in `src/context/`, with
one module per architecture. The original headers remain unmodified in this
vendored tree as upstream references. All other upstream variants remain
available in the vendored tree.
