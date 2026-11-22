//! How far a fade has come.

/// A fade's length; the spec allows 150 to 200 ms.
pub(crate) const FADE_MS: u64 = 180;

/// Opacity from 0 to 255, `elapsed` milliseconds into a fade: quick at first, settling at the end.
pub(crate) fn opacity(elapsed: u64) -> u8 {
    let left = FADE_MS.saturating_sub(elapsed);
    // Ease-out: what remains shrinks with the square of the time left.
    let remaining = 255 * left * left / (FADE_MS * FADE_MS);
    u8::try_from(255 - remaining).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use super::{FADE_MS, opacity};

    #[test]
    fn a_fade_starts_clear_eases_out_and_ends_opaque() {
        assert_eq!(opacity(0), 0);
        assert!(opacity(FADE_MS / 2) > 160, "most of the change comes early");
        assert!(opacity(FADE_MS / 4) < opacity(FADE_MS / 2));
        assert_eq!(opacity(FADE_MS), 255);
        assert_eq!(opacity(FADE_MS * 10), 255);
    }
}
