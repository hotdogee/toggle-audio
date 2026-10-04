//! Build script: embeds `bench/common/detached.manifest` into `ta-rs.exe`.
//!
//! The exe is built for the CONSOLE subsystem (no `windows_subsystem` attribute),
//! so shells wait for it and pipes/redirection work. The manifest sets
//! `consoleAllocationPolicy = detached`, which on Windows 11 24H2+ means a launch
//! from a GUI process (G HUB, Explorer) gets no console window at all, while a
//! launch from a terminal still inherits that terminal's console.
//!
//! The MSVC linker does the embedding itself: `/MANIFEST:EMBED` turns on manifest
//! generation into the `RT_MANIFEST` resource #1, and `/MANIFESTINPUT:<path>` merges
//! our file into it. `/MANIFESTUAC:NO` stops the linker from adding its own
//! `trustInfo` element, because ours already declares `asInvoker`.

use std::env;
use std::path::PathBuf;

fn main() {
    let Some(manifest_dir) = env::var_os("CARGO_MANIFEST_DIR") else {
        panic!("CARGO_MANIFEST_DIR is not set; build.rs must be run by Cargo");
    };
    // bench/rust/../common/detached.manifest, built without `..` components so
    // the linker gets a clean absolute path.
    let manifest = PathBuf::from(manifest_dir)
        .parent()
        .map(|bench| bench.join("common").join("detached.manifest"))
        .unwrap_or_default();
    assert!(
        manifest.is_file(),
        "manifest not found: {}",
        manifest.display()
    );
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rerun-if-changed=build.rs");

    // The flags below are MSVC link.exe syntax; skip them for other linkers.
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        println!("cargo:warning=not an MSVC target: the detached-console manifest is NOT embedded");
        return;
    }
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bins=/MANIFESTUAC:NO");
    println!(
        "cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}",
        manifest.display()
    );
}
