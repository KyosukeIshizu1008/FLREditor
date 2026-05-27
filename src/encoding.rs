use encoding_rs::{SHIFT_JIS, UTF_8};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Encoding {
    #[default]
    ShiftJis,
    Utf8,
    Ascii,
}

impl Encoding {
    pub fn all() -> [Encoding; 3] {
        [Encoding::ShiftJis, Encoding::Utf8, Encoding::Ascii]
    }

    /// Decode bytes to a display string. Invalid sequences are replaced with U+FFFD.
    pub fn decode(self, bytes: &[u8]) -> String {
        match self {
            Encoding::ShiftJis => SHIFT_JIS.decode(bytes).0.into_owned(),
            Encoding::Utf8 => UTF_8.decode(bytes).0.into_owned(),
            Encoding::Ascii => bytes
                .iter()
                .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '·' })
                .collect(),
        }
    }

    /// Encode a string to its exact byte representation in this encoding.
    /// Returns `None` if any character cannot be mapped (e.g. emoji in CP932).
    /// Use this for search patterns where padding would corrupt the query.
    pub fn encode_exact(self, text: &str) -> Option<Vec<u8>> {
        match self {
            Encoding::ShiftJis => {
                let (out, _, had_unmappable) = SHIFT_JIS.encode(text);
                if had_unmappable {
                    return None;
                }
                Some(out.into_owned())
            }
            Encoding::Utf8 => Some(text.as_bytes().to_vec()),
            Encoding::Ascii => {
                if !text.is_ascii() {
                    return None;
                }
                Some(text.as_bytes().to_vec())
            }
        }
    }

    /// Encode a string into a fixed-length byte buffer.
    /// `pad` byte is appended to the right if the result is shorter than `len`.
    /// Returns `None` if the encoded form would overflow `len`.
    pub fn encode_fixed(self, text: &str, len: usize, pad: u8) -> Option<Vec<u8>> {
        let encoded: Vec<u8> = match self {
            Encoding::ShiftJis => {
                let (out, _, had_unmappable) = SHIFT_JIS.encode(text);
                if had_unmappable {
                    return None;
                }
                out.into_owned()
            }
            Encoding::Utf8 => text.as_bytes().to_vec(),
            Encoding::Ascii => {
                if !text.is_ascii() {
                    return None;
                }
                text.as_bytes().to_vec()
            }
        };

        if encoded.len() > len {
            return None;
        }
        let mut buf = encoded;
        buf.resize(len, pad);
        Some(buf)
    }
}

impl fmt::Display for Encoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Encoding::ShiftJis => "Shift_JIS (CP932)",
            Encoding::Utf8 => "UTF-8",
            Encoding::Ascii => "ASCII",
        };
        f.write_str(s)
    }
}
