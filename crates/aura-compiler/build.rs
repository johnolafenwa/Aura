fn main() {
    println!("cargo::rerun-if-changed=build.rs");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "linux" {
        // The coverage FFI integration test resolves its own no-mangle C
        // helpers through RTLD_DEFAULT, matching generated Aura programs.
        // ELF executables expose those symbols only when linked for dynamic
        // export; macOS does this without an additional test-target flag.
        println!("cargo::rustc-link-arg-tests=-Wl,--export-dynamic");
    }
}
