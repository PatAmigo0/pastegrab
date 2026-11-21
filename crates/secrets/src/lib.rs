//! Secrets compiled in at build time from `secrets.toml`, which git ignores: the Telegram bot
//! token, XOR-masked, and your chat ID. `secrets.toml.tmpl` at the repository root shows the format.

#![forbid(unsafe_code)]

mod mask;

use std::fmt;
use std::hint::black_box;

include!(concat!(env!("OUT_DIR"), "/secrets.rs"));

/// The chat that reports go to.
#[derive(Clone, PartialEq, Eq)]
pub struct Telegram {
    token: String,
    chat_id: i64,
}

impl Telegram {
    /// The bot token, for the `bot<token>` part of a Bot API URL. Never log it.
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Your chat with the bot.
    #[must_use]
    pub const fn chat_id(&self) -> i64 {
        self.chat_id
    }
}

impl fmt::Debug for Telegram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Telegram")
            .field("token", &"<hidden>")
            .field("chat_id", &self.chat_id)
            .finish()
    }
}

/// The chat this build reports to, or `None` when it was built without secrets and reports stay
/// on the PC.
#[must_use]
pub fn telegram() -> Option<Telegram> {
    let (token, mask, chat_id) = TELEGRAM?;
    // Without black_box the optimizer could fold the plain token back into the binary.
    let token = String::from_utf8(mask::xor(token, black_box(mask))).ok()?;
    Some(Telegram { token, chat_id })
}

#[cfg(test)]
mod tests {
    #[test]
    fn debug_output_hides_the_token() {
        let t = super::Telegram {
            token: "123456789:secret".into(),
            chat_id: 42,
        };
        let shown = format!("{t:?}");
        assert!(!shown.contains("secret"), "{shown}");
        assert!(shown.contains("42"));
    }
}
