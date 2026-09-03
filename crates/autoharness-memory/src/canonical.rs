use autoharness_domain::Sha256Digest;
use sha2::{Digest as _, Sha256};

use crate::MemoryError;

/// Length-prefixed byte encoder used for stable policy and manifest hashes.
#[derive(Clone, Debug, Default)]
pub struct CanonicalEncoder {
    bytes: Vec<u8>,
}

impl CanonicalEncoder {
    /// Starts an empty canonical value.
    #[must_use]
    pub const fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    /// Appends one named byte field without relying on map ordering.
    pub fn field(&mut self, name: &str, value: &[u8]) -> Result<(), MemoryError> {
        append_length(&mut self.bytes, name.len())?;
        self.bytes.extend_from_slice(name.as_bytes());
        append_length(&mut self.bytes, value.len())?;
        self.bytes.extend_from_slice(value);
        Ok(())
    }

    /// Appends one named unsigned integer in network byte order.
    pub fn integer(&mut self, name: &str, value: u64) -> Result<(), MemoryError> {
        self.field(name, &value.to_be_bytes())
    }

    /// Returns the exact encoded bytes for deterministic comparisons.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Produces a validated lowercase SHA-256 digest.
    pub fn finish(self) -> Result<Sha256Digest, MemoryError> {
        let digest = Sha256::digest(self.bytes);
        let mut hex = String::with_capacity(64);
        for byte in digest {
            use std::fmt::Write as _;
            write!(&mut hex, "{byte:02x}").map_err(|_| MemoryError::InvalidDomainValue)?;
        }
        Sha256Digest::new(hex).map_err(|_| MemoryError::InvalidDomainValue)
    }
}

fn append_length(output: &mut Vec<u8>, length: usize) -> Result<(), MemoryError> {
    let length = u64::try_from(length).map_err(|_| MemoryError::NumericOverflow)?;
    output.extend_from_slice(&length.to_be_bytes());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_boundaries_cannot_collide() {
        let mut first = CanonicalEncoder::new();
        first.field("a", b"bc").expect("field");
        let mut second = CanonicalEncoder::new();
        second.field("ab", b"c").expect("field");

        assert_ne!(first.as_bytes(), second.as_bytes());
        assert_ne!(
            first.finish().expect("digest"),
            second.finish().expect("digest")
        );
    }

    #[test]
    fn digest_shape_is_lowercase_sha256() {
        let mut encoder = CanonicalEncoder::new();
        encoder.field("kind", b"context").expect("field");
        let digest = encoder.finish().expect("digest");

        assert_eq!(digest.as_str().len(), 64);
        assert!(
            digest
                .as_str()
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
    }
}
