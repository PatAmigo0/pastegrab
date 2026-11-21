//! Reads `secrets.toml` from the repository root, or the file `PASTEGRAB_SECRETS` names, and
//! writes its values into `secrets.rs` in `OUT_DIR` with the bot token XOR-masked.

#![allow(
    clippy::print_stdout,
    reason = "Cargo reads build script directives from stdout"
)]

#[path = "src/mask.rs"]
mod mask;

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::{env, fs};

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Secrets {
    telegram: Option<Telegram>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Telegram {
    token: String,
    chat_id: i64,
}

const NONE: &str = "const TELEGRAM: Option<(&[u8], &[u8], i64)> = None;\n";

fn main() {
    println!("cargo::rerun-if-env-changed=PASTEGRAB_SECRETS");
    let Some(out) = env::var_os("OUT_DIR").map(PathBuf::from) else {
        println!("cargo::error=OUT_DIR is not set");
        return;
    };
    let path = env::var_os("PASTEGRAB_SECRETS").map_or_else(default_path, PathBuf::from);
    println!("cargo::rerun-if-changed={}", path.display());
    let code = match read(&path) {
        Ok(Some(telegram)) => masked(&telegram),
        Ok(None) => {
            println!(
                "cargo::warning=no Telegram secrets in {}; reports stay local",
                path.display()
            );
            NONE.to_owned()
        }
        Err(e) => {
            println!("cargo::error={e}");
            NONE.to_owned()
        }
    };
    if let Err(e) = fs::write(out.join("secrets.rs"), code) {
        println!("cargo::error=writing secrets.rs: {e}");
    }
}

fn default_path() -> PathBuf {
    let crate_dir = env::var_os("CARGO_MANIFEST_DIR").map_or_else(PathBuf::new, PathBuf::from);
    let root = crate_dir
        .parent()
        .and_then(Path::parent)
        .unwrap_or(&crate_dir);
    root.join("secrets.toml")
}

/// The Telegram settings, `None` when the file or its section is missing or the token is empty.
/// Errors never quote the token.
fn read(path: &Path) -> Result<Option<Telegram>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let secrets: Secrets = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let Some(telegram) = secrets.telegram.filter(|t| !t.token.is_empty()) else {
        return Ok(None);
    };
    if !looks_like_a_token(&telegram.token) {
        return Err(
            "telegram.token is not a BotFather token: digits, a colon, then the secret".into(),
        );
    }
    if telegram.chat_id == 0 {
        return Err("telegram.chat_id is 0; take it from getUpdates".into());
    }
    Ok(Some(telegram))
}

fn looks_like_a_token(token: &str) -> bool {
    let Some((bot, secret)) = token.split_once(':') else {
        return false;
    };
    (5..=12).contains(&bot.len())
        && bot.bytes().all(|b| b.is_ascii_digit())
        && (30..=64).contains(&secret.len())
        && secret
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The token XOR a fresh mask whose bytes all have the high bit set, so no masked byte is ASCII.
fn masked(telegram: &Telegram) -> String {
    let state = RandomState::new();
    let mask: Vec<u8> = (0..telegram.token.len())
        .map(|i| {
            let mut hasher = state.build_hasher();
            hasher.write_usize(i);
            hasher.finish().to_le_bytes()[0] | 0x80
        })
        .collect();
    let token = mask::xor(telegram.token.as_bytes(), &mask);
    format!(
        "const TELEGRAM: Option<(&[u8], &[u8], i64)> = Some((&{token:?}, &{mask:?}, {}));\n",
        telegram.chat_id
    )
}
