fn main() {
    println!("cargo::rerun-if-changed=build.rs");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "linux" {
        // FFI tests resolve their own no-mangle C helpers through
        // RTLD_DEFAULT, matching generated Aura programs.
        // ELF executables expose those symbols only when linked for dynamic
        // export; macOS does this without an additional test-target flag.
        println!("cargo::rustc-link-arg-tests=-Wl,--export-dynamic");
    }
    if target_os == "macos" {
        // The `tensor` module's Metal device. The shim names no framework
        // class at link time, so the only library it adds is the
        // Objective-C runtime, which every macOS process already holds.
        println!("cargo::rerun-if-changed=src/metal_shim.m");
        cc::Build::new()
            .file("src/metal_shim.m")
            .flag("-fobjc-arc")
            .flag("-Wno-unguarded-availability-new")
            .compile("aura_metal_shim");
        println!("cargo::rustc-link-lib=objc");
    }
}
