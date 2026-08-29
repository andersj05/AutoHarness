use autoharness_domain::EstimatedTokens;

use crate::MemoryError;

/// Versioned deterministic upper-bound estimator for provider context.
pub trait ContextSizer {
    /// Returns the stable algorithm version persisted with each context turn.
    fn version(&self) -> &'static str;

    /// Estimates one complete rendered item including its fixed framing cost.
    fn estimate(&self, rendered: &str) -> Result<EstimatedTokens, MemoryError>;
}

/// Conservative v1 estimator that counts every UTF-8 byte as one token.
#[derive(Clone, Copy, Debug, Default)]
pub struct Utf8ByteSizerV1;

impl ContextSizer for Utf8ByteSizerV1 {
    fn version(&self) -> &'static str {
        "utf8_bytes_v1"
    }

    fn estimate(&self, rendered: &str) -> Result<EstimatedTokens, MemoryError> {
        let bytes = u64::try_from(rendered.len()).map_err(|_| MemoryError::NumericOverflow)?;
        EstimatedTokens::new(bytes).map_err(|_| MemoryError::InvalidDomainValue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_sizer_is_unicode_stable_and_conservative() {
        let sizer = Utf8ByteSizerV1;

        assert_eq!(sizer.version(), "utf8_bytes_v1");
        assert_eq!(sizer.estimate("abc").expect("estimate").get(), 3);
        assert_eq!(sizer.estimate("雪").expect("estimate").get(), 3);
    }
}
