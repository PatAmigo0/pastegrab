//! XOR masking: it keeps the token out of `strings` and stops nobody who tries harder.

/// XORs `data` with `mask`, repeating the mask when it is shorter.
pub(crate) fn xor(data: &[u8], mask: &[u8]) -> Vec<u8> {
    data.iter()
        .zip(mask.iter().cycle())
        .map(|(d, m)| d ^ m)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::xor;

    #[test]
    fn masking_twice_gives_the_input_back() {
        let mask = [0x80, 0xff, 0x91];
        let token = b"123456789:AAE-def_ghi";
        let masked = xor(token, &mask);
        assert!(masked.iter().all(|b| !b.is_ascii_graphic()));
        assert_eq!(xor(&masked, &mask), token);
    }
}
