use std::env;
use std::path::PathBuf;

fn main() {
    let target_architecture =
        env::var("CARGO_CFG_TARGET_ARCH").expect("Cargo supplies a target architecture");
    let target_operating_system =
        env::var("CARGO_CFG_TARGET_OS").expect("Cargo supplies a target OS");
    let target_endianness =
        env::var("CARGO_CFG_TARGET_ENDIAN").expect("Cargo supplies target endianness");
    if !matches!(
        target_operating_system.as_str(),
        "linux" | "macos" | "freebsd"
    ) {
        panic!("socketry-concurrent currently supports Unix targets (Linux, macOS, and FreeBSD)");
    }

    let source = match (
        target_architecture.as_str(),
        target_operating_system.as_str(),
        target_endianness.as_str(),
    ) {
        ("x86_64", _, _) => "amd64/Context.S",
        ("aarch64", _, _) => "arm64/Context.S",
        ("x86", _, _) => "x86/Context.S",
        ("arm", _, _) => "arm32/Context.S",
        ("loongarch64", "linux", _) => "loongarch64/Context.S",
        ("riscv64", "linux" | "freebsd", _) => "riscv64/Context.S",
        ("powerpc", "macos", _) => "ppc/Context.S",
        ("powerpc64", "macos", "big") => "ppc64/Context.S",
        ("powerpc64", "linux", "little") => "ppc64le/Context.S",
        _ => panic!(
            "no integrated CRuby context-switch implementation for target {target_architecture}-{target_operating_system}-{target_endianness}"
        ),
    };

    let source = PathBuf::from("vendor/cruby/coroutine").join(source);
    println!("cargo:rerun-if-changed={}", source.display());
    println!("cargo:rerun-if-changed=vendor/cruby/upstream.md");
    println!("cargo:rustc-check-cfg=cfg(coroutine_shadow_stack)");
    println!("cargo:rustc-check-cfg=cfg(coroutine_address_sanitizer)");
    println!("cargo:rustc-check-cfg=cfg(coroutine_thread_sanitizer)");

    let address_sanitizer = env::var_os("CARGO_FEATURE_ADDRESS_SANITIZER").is_some();
    let thread_sanitizer = env::var_os("CARGO_FEATURE_THREAD_SANITIZER").is_some();
    if address_sanitizer && thread_sanitizer {
        panic!("AddressSanitizer and ThreadSanitizer cannot be enabled together");
    }
    if address_sanitizer {
        println!("cargo:rustc-cfg=coroutine_address_sanitizer");
    }
    if thread_sanitizer {
        println!("cargo:rustc-cfg=coroutine_thread_sanitizer");
    }

    let mut native = cc::Build::new();
    native.file(&source);

    // CRuby lets its build system choose the platform's assembler symbol spelling.
    let prefixed_symbol = if target_operating_system == "macos" {
        "_ ## name"
    } else {
        "name"
    };
    native.flag(&format!("-DPREFIXED_SYMBOL(name)={prefixed_symbol}"));

    // Enable CRuby's runtime-gated CET shadow-stack path on Linux x86-64.
    if target_architecture == "x86_64" && target_operating_system == "linux" {
        native.flag("-fcf-protection=return");
        println!("cargo:rustc-cfg=coroutine_shadow_stack");
    }

    native.compile("socketry_coroutine");
}
