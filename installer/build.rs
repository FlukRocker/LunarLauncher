//! Ensures the payload archive exists so the crate compiles without CI.
//!
//! `include_bytes!` is resolved at compile time, so a missing payload is a
//! build error rather than a runtime one. CI writes the real archive before
//! building; this only creates an empty stand-in so a developer can work on
//! the UI without producing a full launcher build first.

use std::path::Path;

/// Embed the Cyber Launcher icon and version strings into Setup.exe.
///
/// Shares `icon.ico` with the launcher rather than keeping a second copy, so
/// regenerating the app icons cannot leave the installer showing the old mark.
///
/// A failure here is a warning, not an error: the resource compiler is part of
/// the Windows SDK, and a cross-build from Linux may not have one. An installer
/// with a plain icon still installs, whereas a build that refuses to compile
/// because of an icon does not.
#[cfg(target_os = "windows")]
fn embed_windows_resources(version: &str) {
    let icon = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("src-tauri/icons/icon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());

    if !icon.exists() {
        println!("cargo:warning=no icon.ico; Setup.exe keeps the default icon");
        return;
    }

    let mut res = winresource::WindowsResource::new();
    res.set_icon(&icon.to_string_lossy());
    res.set("ProductName", "Cyber Launcher");
    res.set("FileDescription", "Cyber Launcher Setup");
    res.set("CompanyName", "Cyber Network Group");
    res.set(
        "LegalCopyright",
        "Copyright (c) 2026 Cyber Network Group. Portions (c) 2017-2026 Daniel D. Scalzi. MIT.",
    );
    res.set("ProductVersion", version);
    res.set("FileVersion", version);

    if let Err(err) = res.compile() {
        println!("cargo:warning=could not embed Windows resources ({err}); icon not set");
    }
}

#[cfg(not(target_os = "windows"))]
fn embed_windows_resources(_version: &str) {}

fn main() {
    // The version shown to the user is the *launcher's*, read from the same
    // tauri.conf.json the launcher builds from. `CARGO_PKG_VERSION` here is
    // this crate's own, which is unrelated and was being displayed as though
    // it were the product's — a wrong number, presented confidently, on the
    // one screen a user reads before agreeing to install.
    let conf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("src-tauri/tauri.conf.json");
    println!("cargo:rerun-if-changed={}", conf.display());
    let version = std::fs::read_to_string(&conf)
        .ok()
        .and_then(|raw| {
            raw.split("\"version\"")
                .nth(1)?
                .split('"')
                .nth(1)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=LUNAR_VERSION={version}");

    embed_windows_resources(&version);

    let payload = Path::new(env!("CARGO_MANIFEST_DIR")).join("payload.zip");
    println!("cargo:rerun-if-changed={}", payload.display());

    if !payload.exists() {
        // An empty zip: 22 bytes, an end-of-central-directory record and
        // nothing else. Valid enough to open and iterate to zero entries, so
        // the install path fails with "payload is empty" rather than a parse
        // error that reads like corruption.
        const EMPTY_ZIP: [u8; 22] = [
            0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        std::fs::write(&payload, EMPTY_ZIP).expect("write placeholder payload");
        println!("cargo:warning=no payload.zip; built with an empty placeholder");
    }
}
