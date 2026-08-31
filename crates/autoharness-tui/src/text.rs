use std::fmt::Write as _;

use autoharness_domain::is_unsafe_display_control;

/// Escapes terminal and directional control characters before rendering text.
#[must_use]
pub fn display_safe(text: &str) -> String {
    let mut safe = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\n' => safe.push('\n'),
            '\t' => safe.push_str("    "),
            character if is_unsafe_display_control(character) => {
                let _ = write!(safe, "\\u{{{:x}}}", u32::from(character));
            }
            character => safe.push(character),
        }
    }
    safe
}

pub(crate) fn editable_safe(text: &str) -> String {
    display_safe(&text.replace("\r\n", "\n"))
}
