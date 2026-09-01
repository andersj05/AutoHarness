//! Writes or verifies the checked-in GUI theme custom properties.

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let check = arguments
        .next()
        .is_some_and(|argument| argument == "--check");
    let path = arguments.next().map_or_else(
        || PathBuf::from("apps/gui/src/design-system/themes.generated.css"),
        PathBuf::from,
    );
    if arguments.next().is_some() {
        return Err("usage: generate_gui_themes [--check] [output-path]".into());
    }
    let generated = autoharness_presentation::generate_css();
    if check {
        let current = std::fs::read_to_string(&path)?;
        if current != generated {
            return Err(format!("{} is stale; regenerate the theme CSS", path.display()).into());
        }
    } else {
        std::fs::write(&path, generated)?;
    }
    Ok(())
}
