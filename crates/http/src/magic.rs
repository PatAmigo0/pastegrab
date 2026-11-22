/// What the first bytes of a file say it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Media {
    IsoBmff,
    Matroska,
    MpegTs,
    MpegPs,
    Flv,
    Avi,
    Asf,
    Ogg,
    Mp3,
    Aac,
    Flac,
    Wav,
}

/// Bytes the check needs: MPEG-TS repeats its sync byte every 188.
pub(crate) const SNIFF_LEN: usize = 189;

pub(crate) fn sniff(head: &[u8]) -> Option<Media> {
    let at = |offset: usize, bytes: &[u8]| head.get(offset..offset + bytes.len()) == Some(bytes);
    let riff = |kind: &[u8]| at(0, b"RIFF") && at(8, kind);
    let media = if at(4, b"ftyp") {
        Media::IsoBmff
    } else if at(0, &[0x1A, 0x45, 0xDF, 0xA3]) {
        Media::Matroska
    } else if at(0, &[0x47]) && at(188, &[0x47]) {
        Media::MpegTs
    } else if at(0, &[0x00, 0x00, 0x01, 0xBA]) {
        Media::MpegPs
    } else if at(0, b"FLV\x01") {
        Media::Flv
    } else if riff(b"AVI ") {
        Media::Avi
    } else if riff(b"WAVE") {
        Media::Wav
    } else if at(0, &[0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11]) {
        Media::Asf
    } else if at(0, b"OggS") {
        Media::Ogg
    } else if at(0, b"fLaC") {
        Media::Flac
    } else if at(0, b"ID3") {
        Media::Mp3
    } else {
        // An MPEG audio frame header; layer bits 00 mark ADTS AAC.
        match head {
            [0xFF, second, ..] if second & 0xF6 == 0xF0 => Media::Aac,
            [0xFF, second, ..] if second & 0xE0 == 0xE0 => Media::Mp3,
            _ => return None,
        }
    };
    Some(media)
}

impl Media {
    pub(crate) const fn sound_only(self) -> bool {
        matches!(self, Self::Mp3 | Self::Aac | Self::Flac | Self::Wav)
    }
}

#[cfg(test)]
mod tests {
    use super::{Media, sniff};

    #[test]
    fn signatures() {
        let mut ts = vec![0x47; 1];
        ts.resize(188, 0xFF);
        ts.push(0x47);
        let cases: [(&[u8], Option<Media>); 12] = [
            (b"\0\0\0\x18ftypmp42", Some(Media::IsoBmff)),
            (&[0x1A, 0x45, 0xDF, 0xA3, 0x9F], Some(Media::Matroska)),
            (&ts, Some(Media::MpegTs)),
            (&[0x00, 0x00, 0x01, 0xBA, 0x44], Some(Media::MpegPs)),
            (b"FLV\x01\x05", Some(Media::Flv)),
            (b"RIFF\x24\0\0\0AVI LIST", Some(Media::Avi)),
            (b"ID3\x04\0", Some(Media::Mp3)),
            (&[0xFF, 0xFB, 0x90], Some(Media::Mp3)),
            (&[0xFF, 0xF1, 0x50], Some(Media::Aac)),
            (b"<!DOCTYPE html>", None),
            (b"MZ\x90\0", None),
            (&[0xFF, 0xD8, 0xFF, 0xE0], None),
        ];
        for (head, expected) in cases {
            assert_eq!(sniff(head), expected, "{head:x?}");
        }
        assert_eq!(sniff(&ts[..100]), None, "one sync byte is not enough");
    }
}
