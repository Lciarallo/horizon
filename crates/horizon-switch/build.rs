//! On the Switch (`target_os = "horizon"`, aarch64) this compiles `shim/*.c` with devkitA64
//! and links libnx and switch-mesa (EGL, glapi, drm_nouveau). Elsewhere it does nothing.

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=shim");
    println!("cargo:rerun-if-env-changed=DEVKITPRO");
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if os != "horizon" || arch != "aarch64" {
        return;
    }
    let dkp = PathBuf::from(env::var("DEVKITPRO").unwrap_or_else(|_| "/opt/devkitpro".into()));
    let libnx = dkp.join("libnx");
    let portlibs = dkp.join("portlibs/switch");

    let mut build = cc::Build::new();
    build
        .compiler(dkp.join("devkitA64/bin/aarch64-none-elf-gcc"))
        .archiver(dkp.join("devkitA64/bin/aarch64-none-elf-ar"))
        .no_default_flags(true)
        .flag("-march=armv8-a+crc+crypto")
        .flag("-mtune=cortex-a57")
        .flag("-mtp=soft")
        .flag("-fPIE")
        .flag("-O2")
        .flag("-g")
        .flag("-ffunction-sections")
        .flag("-fdata-sections")
        .flag("-Wall")
        .define("__SWITCH__", None)
        .include(libnx.join("include"))
        .include(portlibs.join("include"));
    for entry in std::fs::read_dir("shim").expect("shim directory") {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "c") {
            build.file(path);
        }
    }
    build.compile("horizon_switch_shim");

    println!("cargo:rustc-link-search=native={}", portlibs.join("lib").display());
    println!("cargo:rustc-link-search=native={}", libnx.join("lib").display());
    // switch-mesa's EGL/OpenGL, then libnx. The target spec links with
    // --wrap=pthread_create, which hands every new thread (std's included) to the shim.
    for lib in ["EGL", "glapi", "drm_nouveau", "nx"] {
        println!("cargo:rustc-link-lib=static={lib}");
    }
}
