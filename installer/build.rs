//! Ensures the payload archive exists so the crate compiles without CI.
//!
//! `include_bytes!` is resolved at compile time, so a missing payload is a
//! build error rather than a runtime one. CI writes the real archive before
//! building; this only creates an empty stand-in so a developer can work on
//! the UI without producing a full launcher build first.

use std::path::Path;

fn main() {
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
