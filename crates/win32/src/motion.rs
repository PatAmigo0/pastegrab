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

/// Blends two Win32 `0x00BBGGRR` colors: `alpha == 0` gives `from`, `alpha == 255` gives `to`.
pub(crate) const fn blend(from: u32, to: u32, alpha: u8) -> u32 {
    const fn channel(from: u32, to: u32, alpha: u32, shift: u32) -> u32 {
        let f = (from >> shift) & 0xFF;
        let t = (to >> shift) & 0xFF;
        ((f * (255 - alpha) + t * alpha + 127) / 255) << shift
    }
    let a = alpha as u32;
    channel(from, to, a, 0) | channel(from, to, a, 8) | channel(from, to, a, 16)
}

#[cfg(test)]
mod tests {
    use super::{FADE_MS, blend, opacity};

    #[test]
    fn a_fade_starts_clear_eases_out_and_ends_opaque() {
        assert_eq!(opacity(0), 0);
        assert!(opacity(FADE_MS / 2) > 160, "most of the change comes early");
        assert!(opacity(FADE_MS / 4) < opacity(FADE_MS / 2));
        assert_eq!(opacity(FADE_MS), 255);
        assert_eq!(opacity(FADE_MS * 10), 255);
    }

    #[test]
    fn blending_moves_each_channel_from_background_to_foreground() {
        assert_eq!(blend(0x00FF_FFFF, 0x0028_28A0, 0), 0x00FF_FFFF);
        assert_eq!(blend(0x00FF_FFFF, 0x0028_28A0, 255), 0x0028_28A0);
        assert_eq!(blend(0x0000_0000, 0x00FF_00FF, 128), 0x0080_0080);
    }
}
