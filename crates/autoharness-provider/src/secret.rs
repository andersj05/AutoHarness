use serde_json::Value;
use zeroize::Zeroize as _;

/// Credential-length-bounded observation of provider-authored strings across one turn.
///
/// Only the suffix that could complete a protected form is retained, and it is
/// zeroized when replaced or dropped.
#[doc(hidden)]
pub struct SecretAccumulator {
    ascii_tail: Vec<u8>,
}

impl SecretAccumulator {
    /// Creates an empty turn-scoped observation.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            ascii_tail: Vec::new(),
        }
    }

    /// Adds one text value and checks every protected secret form.
    pub fn observe_text(&mut self, value: &str, secrets: &[&str]) -> bool {
        self.observe_ascii_sequence(value.bytes(), secrets)
    }

    /// Conservatively checks every string key and leaf, then adds string values to the ordered
    /// sequence.
    pub fn observe_structured(&mut self, value: &Value, secrets: &[&str]) -> bool {
        let mut available = [0_usize; 128];
        collect_ascii_string_bytes(value, &mut available);
        if secrets
            .iter()
            .any(|secret| byte_multiset_contains(&available, secret.as_bytes()))
        {
            return true;
        }
        self.observe_structured_sequence(value, secrets)
    }

    fn observe_structured_sequence(&mut self, value: &Value, secrets: &[&str]) -> bool {
        match value {
            Value::String(value) => self.observe_ascii_sequence(value.bytes(), secrets),
            Value::Array(values) => values
                .iter()
                .any(|value| self.observe_structured_sequence(value, secrets)),
            Value::Object(values) => values
                .values()
                .any(|value| self.observe_structured_sequence(value, secrets)),
            Value::Null | Value::Bool(_) | Value::Number(_) => false,
        }
    }

    fn observe_ascii_sequence(
        &mut self,
        bytes: impl Iterator<Item = u8>,
        secrets: &[&str],
    ) -> bool {
        let mut combined = Vec::with_capacity(self.ascii_tail.len());
        combined.extend_from_slice(&self.ascii_tail);
        combined.extend(bytes.filter(u8::is_ascii));
        let contains_secret = secrets.iter().any(|secret| {
            let secret = secret.as_bytes();
            !secret.is_empty()
                && secret.is_ascii()
                && combined
                    .windows(secret.len())
                    .any(|window| window == secret)
        });
        let retained = secrets
            .iter()
            .map(|secret| secret.len().saturating_sub(1))
            .max()
            .unwrap_or(0)
            .min(combined.len());
        self.ascii_tail.zeroize();
        self.ascii_tail = combined.split_off(combined.len().saturating_sub(retained));
        combined.zeroize();
        contains_secret
    }
}

impl Drop for SecretAccumulator {
    fn drop(&mut self) {
        self.ascii_tail.zeroize();
    }
}

impl Default for SecretAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns whether string keys and leaves could reconstruct any complete secret form.
///
/// This conservative boundary check deliberately ignores field ordering and
/// separators. High-entropy credentials split across any number of structured
/// fields must not become provider-neutral or durable data.
#[doc(hidden)]
#[must_use]
pub fn structured_value_may_contain_secret(value: &Value, secrets: &[&str]) -> bool {
    let mut available = [0_usize; 128];
    collect_ascii_string_bytes(value, &mut available);
    secrets
        .iter()
        .any(|secret| byte_multiset_contains(&available, secret.as_bytes()))
}

fn collect_ascii_string_bytes(value: &Value, counts: &mut [usize; 128]) {
    match value {
        Value::String(value) => collect_ascii_bytes(value.bytes(), counts),
        Value::Array(values) => {
            for value in values {
                collect_ascii_string_bytes(value, counts);
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                collect_ascii_bytes(key.bytes(), counts);
                collect_ascii_string_bytes(value, counts);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn collect_ascii_bytes(bytes: impl Iterator<Item = u8>, counts: &mut [usize; 128]) {
    for byte in bytes.filter(u8::is_ascii) {
        counts[usize::from(byte)] = counts[usize::from(byte)].saturating_add(1);
    }
}

fn byte_multiset_contains(available: &[usize; 128], secret: &[u8]) -> bool {
    if secret.is_empty() || !secret.is_ascii() {
        return false;
    }
    let mut required = [0_usize; 128];
    for &byte in secret {
        let index = usize::from(byte);
        required[index] = required[index].saturating_add(1);
        if required[index] > available[index] {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn detects_secret_fragments_across_fields_arrays_and_orders() {
        let value = json!({
            "path": "second-half",
            "arguments": ["noise", "first-half"]
        });

        assert!(structured_value_may_contain_secret(
            &value,
            &["first-halfsecond-half"]
        ));
        assert!(!structured_value_may_contain_secret(
            &value,
            &["different-secret"]
        ));
    }

    #[test]
    fn detects_secret_fragments_across_an_object_key_and_value() {
        let value = json!({"key-fragment":"value-fragment"});

        assert!(structured_value_may_contain_secret(
            &value,
            &["key-fragmentvalue-fragment"]
        ));
        let mut accumulator = SecretAccumulator::new();
        assert!(accumulator.observe_structured(&value, &["key-fragmentvalue-fragment"]));
    }

    #[test]
    fn accumulator_detects_secret_only_after_multiple_events() {
        let mut accumulator = SecretAccumulator::new();

        assert!(!accumulator.observe_text("first-half", &["first-halfsecond-half"]));
        assert!(
            accumulator
                .observe_structured(&json!({"path":"second-half"}), &["first-halfsecond-half"])
        );
    }
}
