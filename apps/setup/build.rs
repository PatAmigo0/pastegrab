//! Version info and a manifest for the setup, and what it carries: the payload `PASTEGRAB_PAYLOAD`
//! names and the redistributable `PASTEGRAB_REDIST` names, each nothing in a development build.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    pastegrab_winres::embed(
        pastegrab_winres::Kind::Setup,
        "pastegrab-setup.exe",
        "pastegrab setup",
    );
    println!("cargo::rerun-if-env-changed=PASTEGRAB_PAYLOAD_SIZE");
    carry("PASTEGRAB_PAYLOAD", "PASTEGRAB_PAYLOAD_FILE");
    carry("PASTEGRAB_REDIST", "PASTEGRAB_REDIST_FILE");
    let size = env::var("PASTEGRAB_PAYLOAD_SIZE").unwrap_or_else(|_| "0".into());
    println!("cargo::rustc-env=PASTEGRAB_PAYLOAD_SIZE={size}");
}

/// Hands the file `input` names, or an empty one, to the crate as `output`.
fn carry(input: &str, output: &str) {
    println!("cargo::rerun-if-env-changed={input}");
    let file = if let Some(file) = env::var_os(input) {
        PathBuf::from(file)
    } else {
        let out = PathBuf::from(env::var_os("OUT_DIR").unwrap_or_default());
        let empty = out.join(format!("{output}.empty"));
        if let Err(e) = fs::write(&empty, []) {
            println!("cargo::error={}: {e}", empty.display());
        }
        empty
    };
    println!("cargo::rerun-if-changed={}", file.display());
    println!("cargo::rustc-env={output}={}", file.display());
}
