use std::env;
use std::path::PathBuf;

fn main() {
    let target_architecture =
        env::var("CARGO_CFG_TARGET_ARCH").expect("Cargo supplies a target architecture");
    let target_operating_system =
        env::var("CARGO_CFG_TARGET_OS").expect("Cargo supplies a target OS");
    let target_endianness =
        env::var("CARGO_CFG_TARGET_ENDIAN").expect("Cargo supplies target endianness");
    let target_pointer_width = env::var("CARGO_CFG_TARGET_POINTER_WIDTH")
        .expect("Cargo supplies the target pointer width");

    if !matches!(
        target_operating_system.as_str(),
        "linux" | "macos" | "freebsd"
    ) {
        panic!("socketry-concurrent currently supports Unix targets (Linux, macOS, and FreeBSD)");
    }

    let (source, context_architecture) = match (
        target_architecture.as_str(),
        target_operating_system.as_str(),
        target_endianness.as_str(),
    ) {
        ("x86_64", _, _) => ("amd64/Context.S", "AMD64"),
        ("aarch64", _, _) => ("arm64/Context.S", "ARM64"),
        ("x86", _, _) => ("x86/Context.S", "X86"),
        ("arm", _, _) => ("arm32/Context.S", "ARM32"),
        ("loongarch64", "linux", _) => ("loongarch64/Context.S", "LOONGARCH64"),
        ("riscv64", "linux" | "freebsd", _) => ("riscv64/Context.S", "RISCV64"),
        ("powerpc", "macos", _) => ("ppc/Context.S", "PPC"),
        ("powerpc64", "macos", "big") => ("ppc64/Context.S", "PPC64"),
        ("powerpc64", "linux", "little") => ("ppc64le/Context.S", "PPC64LE"),
        _ => panic!(
            "no integrated CRuby context-switch implementation for target {target_architecture}-{target_operating_system}-{target_endianness}"
        ),
    };

    let source = PathBuf::from("vendor/cruby/coroutine").join(source);
    let context_shim = PathBuf::from("src/context_shim.c");
    println!("cargo:rerun-if-changed={}", source.display());
    println!("cargo:rerun-if-changed={}", context_shim.display());
    println!(
        "cargo:rerun-if-changed=vendor/cruby/coroutine/{}/Context.h",
        context_architecture.to_lowercase()
    );
    println!("cargo:rerun-if-changed=vendor/cruby/upstream.md");

    let mut native = cc::Build::new();
    native
        .file(&source)
        .file(&context_shim)
        .include("vendor/cruby/coroutine")
        .define(&format!("SOCKETRY_CONTEXT_{context_architecture}"), None);

    let context_storage_size = (16
        * target_pointer_width
            .parse::<usize>()
            .expect("target pointer width is a number")
        / 8)
    .to_string();
    native.define(
        "SOCKETRY_CONTEXT_STORAGE_SIZE",
        Some(context_storage_size.as_str()),
    );

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
    }

    native.compile("socketry_coroutine");
}
