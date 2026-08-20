use std::fmt::Write as _;

/// Escapes terminal and directional control characters before rendering text.
#[must_use]
pub fn display_safe(text: &str) -> String {
    let mut safe = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\n' => safe.push('\n'),
            '\t' => safe.push_str("    "),
            character if is_unsafe_control(character) => {
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

fn is_unsafe_control(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '\u{061c}'
                | '\u{200e}'
                | '\u{200f}'
                | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
                | '\u{fff9}'..='\u{fffb}'
        )
}
