use std::env;
use std::path::PathBuf;

fn main() {
    let arch = env::var("CARGO_CFG_TARGET_ARCH").expect("Cargo supplies a target architecture");
    let os = env::var("CARGO_CFG_TARGET_OS").expect("Cargo supplies a target OS");

    if !matches!(os.as_str(), "linux" | "macos" | "freebsd") {
        panic!("socketry-concurrent currently supports Unix targets (Linux, macOS, and FreeBSD)");
    }

    let source = match arch.as_str() {
        "x86_64" => "vendor/cruby/coroutine/amd64/Context.S",
        "aarch64" => "vendor/cruby/coroutine/arm64/Context.S",
        _ => panic!("no integrated CRuby context-switch implementation for target {arch}-{os}"),
    };

    let source = PathBuf::from(source);
    println!("cargo:rerun-if-changed={}", source.display());
    println!("cargo:rerun-if-changed=vendor/cruby/upstream.md");

    let mut assembly = cc::Build::new();
    assembly.file(&source);

    // CRuby lets its build system choose the platform's assembler symbol spelling.
    let prefixed_symbol = if os == "macos" { "_ ## name" } else { "name" };
    assembly.flag(&format!("-DPREFIXED_SYMBOL(name)={prefixed_symbol}"));

    // Enable CRuby's runtime-gated CET shadow-stack path on Linux x86-64.
    if arch == "x86_64" && os == "linux" {
        assembly.flag("-fcf-protection=return");
    }

    assembly.compile("socketry_coroutine");
}
