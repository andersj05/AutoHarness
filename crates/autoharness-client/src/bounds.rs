use autoharness_domain::contains_unsafe_display_control;

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
/// Maximum UTF-8 bytes in one exact security-critical permission field.
pub const MAX_PERMISSION_DETAIL_BYTES: usize = 512 * 1024;
/// Maximum aggregate UTF-8 bytes across one exact permission request.
pub const MAX_PERMISSION_TOTAL_BYTES: usize = 512 * 1024;
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
///
/// Built-in process calls can carry 256 arguments plus trusted program and directory fields.
pub const MAX_PERMISSION_DETAILS: usize = 260;
/// Maximum selectable rows in one provider-neutral catalog.
pub const MAX_CATALOG_MODELS: usize = 4_096;
/// Maximum provider states in one complete client snapshot.
pub const MAX_PROVIDERS: usize = 64;
/// Maximum visible-ASCII bytes accepted by dedicated secret ingress.
pub const MAX_CREDENTIAL_BYTES: usize = 4 * 1024;

pub(crate) fn validate_credential(value: &str) -> Result<(), ValidationError> {
    if value.is_empty() {
        return Err(ValidationError::Empty {
            field: "credential",
        });
    }
    if value.len() > MAX_CREDENTIAL_BYTES {
        return Err(ValidationError::TooLong {
            field: "credential",
            max_bytes: MAX_CREDENTIAL_BYTES,
            actual_bytes: value.len(),
        });
    }
    if !value.chars().all(|character| character.is_ascii_graphic()) {
        return Err(ValidationError::Invalid {
            field: "credential",
        });
    }
    Ok(())
}

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

pub(crate) fn validate_non_empty_security_text(
    field: &'static str,
    value: &str,
    max_bytes: usize,
) -> Result<(), ValidationError> {
    validate_non_empty_text(field, value, max_bytes)?;
    validate_security_text(field, value, max_bytes)
}

pub(crate) fn validate_security_text(
    field: &'static str,
    value: &str,
    max_bytes: usize,
) -> Result<(), ValidationError> {
    validate_text(field, value, max_bytes)?;
    if contains_unsafe_display_control(value) {
        return Err(ValidationError::Invalid { field });
    }
    Ok(())
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
