use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use thiserror::Error;

const SHA256_BYTES: usize = 32;
const SHA256_HEX_CHARS: usize = SHA256_BYTES * 2;

/// A SHA-256 content identity.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Sha256Digest([u8; SHA256_BYTES]);

impl Sha256Digest {
    /// Creates a digest from its raw bytes.
    pub const fn from_bytes(bytes: [u8; SHA256_BYTES]) -> Self {
        Self(bytes)
    }

    /// Returns the raw digest bytes.
    pub const fn as_bytes(&self) -> &[u8; SHA256_BYTES] {
        &self.0
    }

    /// Returns the lowercase hexadecimal digest without an algorithm prefix.
    pub fn to_hex(self) -> String {
        let mut output = String::with_capacity(SHA256_HEX_CHARS);
        for byte in self.0 {
            use fmt::Write;
            write!(output, "{byte:02x}").expect("writing to a String cannot fail");
        }
        output
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("sha256:")?;
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for Sha256Digest {
    type Err = DigestError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let hex = value.strip_prefix("sha256:").unwrap_or(value);
        if hex.len() != SHA256_HEX_CHARS {
            return Err(DigestError::InvalidLength(hex.len()));
        }

        let mut bytes = [0_u8; SHA256_BYTES];
        for (index, byte) in bytes.iter_mut().enumerate() {
            let offset = index * 2;
            *byte = u8::from_str_radix(&hex[offset..offset + 2], 16)
                .map_err(|_| DigestError::InvalidHex(offset))?;
        }
        Ok(Self(bytes))
    }
}

impl Serialize for Sha256Digest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Sha256Digest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}

/// Validation failures for textual content digests.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum DigestError {
    /// The digest did not contain 64 hexadecimal characters.
    #[error("SHA-256 digest has {0} hexadecimal characters; expected 64")]
    InvalidLength(usize),
    /// A non-hexadecimal character was found at the byte offset.
    #[error("SHA-256 digest contains invalid hexadecimal data at offset {0}")]
    InvalidHex(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_and_parse_round_trip() {
        let digest = Sha256Digest::from_bytes([0xab; SHA256_BYTES]);
        assert_eq!(digest.to_string().parse(), Ok(digest));
    }

    #[test]
    fn rejects_short_digest() {
        assert_eq!(
            "ab".parse::<Sha256Digest>(),
            Err(DigestError::InvalidLength(2))
        );
    }

    #[test]
    fn serde_uses_traceable_algorithm_prefixed_text() {
        let digest = Sha256Digest::from_bytes([0xab; SHA256_BYTES]);
        let json = serde_json::to_string(&digest).unwrap();
        assert_eq!(json, format!("\"{digest}\""));
        assert_eq!(serde_json::from_str::<Sha256Digest>(&json).unwrap(), digest);
    }
}
