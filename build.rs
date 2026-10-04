//! Build script: compiles the Windows resources (manifest, icon, settings dialog, VERSIONINFO) with
//! the Windows SDK resource compiler, once per executable: `assets/app.rc` for `toggle-audio.exe`
//! and `assets/app-windowed.rc` (which includes `app.rc` with its own file name) for
//! `toggle-audiow.exe`.

use std::env;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    // embed-resource emits rerun-if-changed for the .rc itself; the files it pulls in are listed here.
    for input in [
        "assets/app.rc",
        "assets/app-windowed.rc",
        "assets/app.manifest",
        "assets/icon/toggle-audio.ico",
        "assets/resource.h",
    ] {
        println!("cargo:rerun-if-changed={input}");
    }

    // Pass the Cargo version into the .rc so VERSIONINFO never drifts from Cargo.toml.
    let macros = [
        format!("VER_MAJOR={}", env::var("CARGO_PKG_VERSION_MAJOR")?),
        format!("VER_MINOR={}", env::var("CARGO_PKG_VERSION_MINOR")?),
        format!("VER_PATCH={}", env::var("CARGO_PKG_VERSION_PATCH")?),
    ];

    // One compiled resource per binary (cargo:rustc-link-arg-bin), so each carries its own
    // OriginalFilename; app-windowed.rc includes app.rc. Tests do not link resources.
    // The manifest is required: without it the dialog loses visual styles and DPI awareness, and
    // toggle-audio.exe loses the detached console allocation policy.
    embed_resource::compile_for("assets/app.rc", ["toggle-audio"], &macros).manifest_required()?;
    embed_resource::compile_for("assets/app-windowed.rc", ["toggle-audiow"], &macros)
        .manifest_required()?;
    Ok(())
}
