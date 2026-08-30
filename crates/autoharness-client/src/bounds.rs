use crate::ValidationError;

/// Maximum UTF-8 bytes in any stable public identity.
pub const MAX_ID_BYTES: usize = 512;
/// Maximum UTF-8 bytes accepted for one submitted prompt.
pub const MAX_PROMPT_BYTES: usize = 128 * 1024;
/// Maximum UTF-8 bytes retained in one transcript content value.
pub const MAX_TRANSCRIPT_CONTENT_BYTES: usize = 1024 * 1024;
/// Maximum UTF-8 bytes in one session title.
pub const MAX_SESSION_TITLE_BYTES: usize = 256;
/// Maximum UTF-8 bytes in a short public label.
pub const MAX_LABEL_BYTES: usize = 512;
/// Maximum UTF-8 bytes in one safe detail or summary.
pub const MAX_DETAIL_BYTES: usize = 4 * 1024;
/// Maximum UTF-8 bytes in a public safe-failure message.
pub const MAX_FAILURE_MESSAGE_BYTES: usize = 4 * 1024;
/// Maximum UTF-8 bytes in a stable failure code.
pub const MAX_FAILURE_CODE_BYTES: usize = 64;
/// Maximum host-advertised delay before a retry becomes eligible.
pub const MAX_RETRY_DELAY_MS: u64 = 7 * 24 * 60 * 60 * 1_000;
/// Maximum sessions represented by one complete client snapshot.
pub const MAX_SESSIONS: usize = 4_096;
/// Maximum transcript rows represented by one active-session snapshot.
pub const MAX_TRANSCRIPT_ITEMS: usize = 65_536;
/// Maximum unresolved permission requests in one session snapshot.
pub const MAX_PERMISSION_REQUESTS: usize = 256;
/// Maximum trusted operation details in one permission request.
pub const MAX_PERMISSION_DETAILS: usize = 32;
/// Maximum selectable rows in one provider-neutral catalog.
pub const MAX_CATALOG_MODELS: usize = 4_096;
/// Maximum provider states in one complete client snapshot.
pub const MAX_PROVIDERS: usize = 64;
/// Maximum UTF-8 bytes accepted by dedicated secret ingress.
pub const MAX_CREDENTIAL_BYTES: usize = 16 * 1024;

pub(crate) fn validate_non_empty_text(
    field: &'static str,
    value: &str,
    max_bytes: usize,
) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        return Err(ValidationError::Empty { field });
    }
    validate_text(field, value, max_bytes)
}

pub(crate) fn validate_text(
    field: &'static str,
    value: &str,
    max_bytes: usize,
) -> Result<(), ValidationError> {
    if value.len() > max_bytes {
        return Err(ValidationError::TooLong {
            field,
            max_bytes,
            actual_bytes: value.len(),
        });
    }
    Ok(())
}

pub(crate) fn validate_id(field: &'static str, value: &str) -> Result<(), ValidationError> {
    validate_non_empty_text(field, value, MAX_ID_BYTES)?;
    if value.trim() != value || value.chars().any(char::is_control) {
        return Err(ValidationError::Invalid { field });
    }
    Ok(())
}

pub(crate) fn validate_count(
    field: &'static str,
    actual_items: usize,
    max_items: usize,
) -> Result<(), ValidationError> {
    if actual_items > max_items {
        return Err(ValidationError::TooMany {
            field,
            max_items,
            actual_items,
        });
    }
    Ok(())
}
