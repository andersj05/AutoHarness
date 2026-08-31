use std::fmt::Write as _;

/// Returns whether text contains a control that can conceal or reorder security-critical text.
#[must_use]
pub fn contains_unsafe_display_control(text: &str) -> bool {
    text.chars().any(is_unsafe_display_control)
}

/// Escapes controls that can conceal or reorder security-critical text.
#[must_use]
pub fn security_display_safe(text: &str) -> String {
    let mut safe = String::with_capacity(text.len());
    for character in text.chars() {
        if character == '\\' {
            safe.push_str("\\\\");
        } else if is_unsafe_display_control(character) {
            let _ = write!(safe, "\\u{{{:x}}}", u32::from(character));
        } else {
            safe.push(character);
        }
    }
    safe
}

/// Returns whether one character can conceal or reorder security-critical text.
#[must_use]
pub fn is_unsafe_display_control(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{00ad}'
                | '\u{034f}'
                | '\u{061c}'
                | '\u{115f}'..='\u{1160}'
                | '\u{17b4}'..='\u{17b5}'
                | '\u{180b}'..='\u{180f}'
                | '\u{200b}'..='\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2060}'..='\u{206f}'
                | '\u{3164}'
                | '\u{fe00}'..='\u{fe0f}'
                | '\u{feff}'
                | '\u{ffa0}'
                | '\u{fff0}'..='\u{fffb}'
                | '\u{1bca0}'..='\u{1bca3}'
                | '\u{1d173}'..='\u{1d17a}'
                | '\u{e0000}'..='\u{e0fff}'
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_display_text_exposes_directional_and_control_characters() {
        let unsafe_text = "safe\u{202e}txt.exe\u{200b}\nnext";

        assert!(contains_unsafe_display_control(unsafe_text));
        assert_eq!(
            security_display_safe(unsafe_text),
            "safe\\u{202e}txt.exe\\u{200b}\\u{a}next"
        );
        assert!(!contains_unsafe_display_control(&security_display_safe(
            unsafe_text
        )));

        let encoded_control = security_display_safe("safe\u{202e}txt.exe");
        let encoded_literal = security_display_safe("safe\\u{202e}txt.exe");
        assert_eq!(encoded_control, "safe\\u{202e}txt.exe");
        assert_eq!(encoded_literal, "safe\\\\u{202e}txt.exe");
        assert_ne!(encoded_control, encoded_literal);
    }
}
