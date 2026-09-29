#include <stddef.h>

#if defined(__GNUC__)
#pragma GCC diagnostic push
#pragma GCC diagnostic ignored "-Wunused-parameter"
#endif

#if defined(SOCKETRY_CONTEXT_AMD64)
#include "amd64/Context.h"
#elif defined(SOCKETRY_CONTEXT_ARM64)
#include "arm64/Context.h"
#elif defined(SOCKETRY_CONTEXT_ARM32)
#include "arm32/Context.h"
#elif defined(SOCKETRY_CONTEXT_LOONGARCH64)
#include "loongarch64/Context.h"
#elif defined(SOCKETRY_CONTEXT_PPC)
#include "ppc/Context.h"
#elif defined(SOCKETRY_CONTEXT_PPC64)
#include "ppc64/Context.h"
#elif defined(SOCKETRY_CONTEXT_PPC64LE)
#include "ppc64le/Context.h"
#elif defined(SOCKETRY_CONTEXT_RISCV64)
#include "riscv64/Context.h"
#elif defined(SOCKETRY_CONTEXT_X86)
#include "x86/Context.h"
#else
#error "No CRuby coroutine context header for this architecture"
#endif

#if defined(__GNUC__)
#pragma GCC diagnostic pop
#endif

#define SOCKETRY_CONTEXT_STORAGE_ALIGNMENT 16

_Static_assert(
    sizeof(struct coroutine_context) <= SOCKETRY_CONTEXT_STORAGE_SIZE,
    "Rust coroutine context storage is too small"
);
_Static_assert(
    _Alignof(struct coroutine_context) <= SOCKETRY_CONTEXT_STORAGE_ALIGNMENT,
    "Rust coroutine context storage is under-aligned"
);

void socketry_coroutine_context_initialize_main(void *storage)
{
    coroutine_initialize_main((struct coroutine_context *)storage);
}

void socketry_coroutine_context_initialize(
    void *storage,
    coroutine_start start,
    void *stack,
    size_t size
)
{
    coroutine_initialize((struct coroutine_context *)storage, start, stack, size);
}

void socketry_coroutine_context_destroy(void *storage)
{
    coroutine_destroy((struct coroutine_context *)storage);
}
