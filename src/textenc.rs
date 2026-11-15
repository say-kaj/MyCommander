//! Text encodings for Viewer, Editor and Quick view (Left/Right ▸ Encoding…).
//! Auto picks UTF-8 (or a BOM'd UTF-8/UTF-16) and falls back to Windows-1252.
//! The Editor saves in the encoding the file was read with.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextEncoding {
    #[default]
    Auto,
    Utf8,
    Windows1252,
    Iso8859_15,
    MacRoman,
    Windows1250,
    Windows1251,
    Koi8R,
    Utf16Le,
    Utf16Be,
}

impl TextEncoding {
    pub const ALL: [TextEncoding; 10] = [
        TextEncoding::Auto,
        TextEncoding::Utf8,
        TextEncoding::Windows1252,
        TextEncoding::Iso8859_15,
        TextEncoding::MacRoman,
        TextEncoding::Windows1250,
        TextEncoding::Windows1251,
        TextEncoding::Koi8R,
        TextEncoding::Utf16Le,
        TextEncoding::Utf16Be,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TextEncoding::Auto => "Automatic (UTF-8, else Windows-1252)",
            TextEncoding::Utf8 => "UTF-8",
            TextEncoding::Windows1252 => "Western – Windows-1252 / ISO-8859-1",
            TextEncoding::Iso8859_15 => "Western – ISO-8859-15",
            TextEncoding::MacRoman => "Western – Mac Roman",
            TextEncoding::Windows1250 => "Central European – Windows-1250",
            TextEncoding::Windows1251 => "Cyrillic – Windows-1251",
            TextEncoding::Koi8R => "Cyrillic – KOI8-R",
            TextEncoding::Utf16Le => "UTF-16 little endian",
            TextEncoding::Utf16Be => "UTF-16 big endian",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            TextEncoding::Auto => "auto",
            TextEncoding::Utf8 => "UTF-8",
            TextEncoding::Windows1252 => "Windows-1252",
            TextEncoding::Iso8859_15 => "ISO-8859-15",
            TextEncoding::MacRoman => "Mac Roman",
            TextEncoding::Windows1250 => "Windows-1250",
            TextEncoding::Windows1251 => "Windows-1251",
            TextEncoding::Koi8R => "KOI8-R",
            TextEncoding::Utf16Le => "UTF-16LE",
            TextEncoding::Utf16Be => "UTF-16BE",
        }
    }

    fn codec(self) -> &'static encoding_rs::Encoding {
        use encoding_rs as e;
        match self {
            TextEncoding::Auto | TextEncoding::Utf8 => e::UTF_8,
            TextEncoding::Windows1252 => e::WINDOWS_1252,
            TextEncoding::Iso8859_15 => e::ISO_8859_15,
            TextEncoding::MacRoman => e::MACINTOSH,
            TextEncoding::Windows1250 => e::WINDOWS_1250,
            TextEncoding::Windows1251 => e::WINDOWS_1251,
            TextEncoding::Koi8R => e::KOI8_R,
            TextEncoding::Utf16Le => e::UTF_16LE,
            TextEncoding::Utf16Be => e::UTF_16BE,
        }
    }

    fn is_utf16(self) -> bool {
        matches!(self, TextEncoding::Utf16Le | TextEncoding::Utf16Be)
    }
}

pub struct Decoded {
    pub text: String,
    /// The encoding actually used (never `Auto`).
    pub enc: TextEncoding,
    /// The file started with a byte order mark (kept when saving).
    pub bom: bool,
}

/// Sniff a byte order mark.
fn bom_of(b: &[u8]) -> Option<(TextEncoding, usize)> {
    if b.starts_with(&[0xEF, 0xBB, 0xBF]) {
        Some((TextEncoding::Utf8, 3))
    } else if b.starts_with(&[0xFF, 0xFE]) {
        Some((TextEncoding::Utf16Le, 2))
    } else if b.starts_with(&[0xFE, 0xFF]) {
        Some((TextEncoding::Utf16Be, 2))
    } else {
        None
    }
}

/// True if the bytes should be shown as a hex dump (NUL bytes, unless the text is UTF-16).
pub fn looks_binary(bytes: &[u8], want: TextEncoding) -> bool {
    if want.is_utf16() || matches!(bom_of(bytes), Some((e, _)) if e.is_utf16()) && want == TextEncoding::Auto {
        return false;
    }
    bytes.iter().take(4096).any(|&b| b == 0)
}

pub fn decode(bytes: &[u8], want: TextEncoding) -> Decoded {
    let (enc, skip, bom) = match (want, bom_of(bytes)) {
        (TextEncoding::Auto, Some((e, n))) => (e, n, true),
        (TextEncoding::Auto, None) => {
            if std::str::from_utf8(bytes).is_ok() {
                (TextEncoding::Utf8, 0, false)
            } else {
                (TextEncoding::Windows1252, 0, false)
            }
        }
        (w, Some((e, n))) if e == w => (w, n, true),
        (w, _) => (w, 0, false),
    };
    let (text, _) = enc.codec().decode_without_bom_handling(&bytes[skip..]);
    Decoded { text: text.into_owned(), enc, bom }
}

pub fn encode(text: &str, enc: TextEncoding, bom: bool) -> Vec<u8> {
    let mut out = Vec::new();
    match enc {
        TextEncoding::Utf16Le | TextEncoding::Utf16Be => {
            let le = enc == TextEncoding::Utf16Le;
            if bom {
                out.extend(if le { [0xFF, 0xFE] } else { [0xFE, 0xFF] });
            }
            for u in text.encode_utf16() {
                out.extend(if le { u.to_le_bytes() } else { u.to_be_bytes() });
            }
        }
        TextEncoding::Auto | TextEncoding::Utf8 => {
            if bom {
                out.extend([0xEF, 0xBB, 0xBF]);
            }
            out.extend(text.as_bytes());
        }
        e => out.extend(e.codec().encode(text).0.iter()),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_detects() {
        assert_eq!(decode("Grüße".as_bytes(), TextEncoding::Auto).enc, TextEncoding::Utf8);
        let latin = [b'G', b'r', 0xFC, 0xDF, b'e']; // Grüße in Windows-1252
        let d = decode(&latin, TextEncoding::Auto);
        assert_eq!((d.text.as_str(), d.enc), ("Grüße", TextEncoding::Windows1252));
        let bom = [0xEF, 0xBB, 0xBF, b'a'];
        let d = decode(&bom, TextEncoding::Auto);
        assert_eq!((d.text.as_str(), d.bom), ("a", true));
    }

    #[test]
    fn utf16_and_roundtrip() {
        let bytes = encode("Hallo Ä", TextEncoding::Utf16Le, true);
        assert!(!looks_binary(&bytes, TextEncoding::Auto));
        let d = decode(&bytes, TextEncoding::Auto);
        assert_eq!((d.text.as_str(), d.enc, d.bom), ("Hallo Ä", TextEncoding::Utf16Le, true));
        assert_eq!(encode(&d.text, d.enc, d.bom), bytes);
        for e in [TextEncoding::Windows1252, TextEncoding::Iso8859_15, TextEncoding::MacRoman, TextEncoding::Windows1251, TextEncoding::Koi8R] {
            let b = encode("aö", e, false);
            let back = decode(&b, e);
            assert_eq!(back.text.chars().next(), Some('a'));
        }
        assert_eq!(decode(&[0xE4], TextEncoding::MacRoman).text, "‰");
        assert!(looks_binary(&[1, 0, 2], TextEncoding::Auto));
    }
}
