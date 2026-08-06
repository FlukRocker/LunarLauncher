//! Bakes build-time configuration into the binary.
//!
//! Two audiences:
//!
//!  * A developer testing locally, who wants `LUNAR_DISTRO_URL` set once in a
//!    `.env` rather than exported before every command.
//!  * CyberLauncherController, which builds a branded launcher per customer
//!    and cannot rely on the machine that *runs* the launcher having any
//!    environment set at all. Those values must be compiled in.
//!
//! Both are the same mechanism: read `.env`, let the real environment win,
//! emit `cargo:rustc-env` so `option_env!` sees the result. What is baked is
//! still overridable at runtime — see `lib.rs` for the precedence chain.

// std-only, and no `crate::` references; see the module's own note.
include!("src/env_file.rs");

/// The variables a branded build may bake in. Adding one here is all that is
/// needed for `option_env!("LUNAR_…")` to see it.
const BAKED: &[&str] = &[
    "LUNAR_DISTRO_URL",
    "LUNAR_AUTH_SERVER",
    "LUNAR_BRAND_NAME",
    "LUNAR_AZURE_CLIENT_ID",
    "LUNAR_UPDATE_CHANNEL",
];

fn main() {
    // `.env` at the repo root, one level up from src-tauri. Cargo sets
    // CARGO_MANIFEST_DIR regardless of where the build was invoked from, so
    // this does not depend on the caller's working directory.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();

    // `.env.local` first: `apply` does not overwrite, so the file listed
    // earlier wins. That makes .local the personal override of a shared .env.
    for name in [".env.local", ".env"] {
        let path = root.join(name);
        // Announced even when absent, because *creating* the file has to
        // trigger a rebuild and Cargo only watches paths it is told about.
        println!("cargo:rerun-if-changed={}", path.display());
        apply(&path);
    }

    for key in BAKED {
        println!("cargo:rerun-if-env-changed={key}");
        if let Ok(value) = std::env::var(key) {
            if !value.trim().is_empty() {
                // A newline would end the directive early and bake a silently
                // truncated value rather than failing.
                let value = value.trim().replace(['\n', '\r'], "");
                println!("cargo:rustc-env={key}={value}");
            }
        }
    }

    tauri_build::build()
}
