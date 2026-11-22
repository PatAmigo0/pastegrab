//! Version info and a manifest for the setup, and the payload it carries: the file
//! `PASTEGRAB_PAYLOAD` names, or nothing in a development build.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    pastegrab_winres::embed(
        pastegrab_winres::Kind::Setup,
        "pastegrab-setup.exe",
        "pastegrab setup",
    );
    println!("cargo::rerun-if-env-changed=PASTEGRAB_PAYLOAD");
    println!("cargo::rerun-if-env-changed=PASTEGRAB_PAYLOAD_SIZE");
    let payload = if let Some(file) = env::var_os("PASTEGRAB_PAYLOAD") {
        PathBuf::from(file)
    } else {
        let out = PathBuf::from(env::var_os("OUT_DIR").unwrap_or_default());
        let empty = out.join("empty.tar.zst");
        if let Err(e) = fs::write(&empty, []) {
            println!("cargo::error={}: {e}", empty.display());
        }
        empty
    };
    println!("cargo::rerun-if-changed={}", payload.display());
    println!(
        "cargo::rustc-env=PASTEGRAB_PAYLOAD_FILE={}",
        payload.display()
    );
    let size = env::var("PASTEGRAB_PAYLOAD_SIZE").unwrap_or_else(|_| "0".into());
    println!("cargo::rustc-env=PASTEGRAB_PAYLOAD_SIZE={size}");
}
