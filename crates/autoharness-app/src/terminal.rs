use std::io;

#[cfg(any(not(windows), test))]
use std::io::Write;

use crate::error::AppError;
use crossterm::cursor::Show;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

#[cfg(windows)]
type AppTerminal = ratatui::DefaultTerminal;

#[cfg(not(windows))]
type AppTerminal = Terminal<CrosstermBackend<Ansi16Writer<io::Stdout>>>;

#[cfg(any(not(windows), test))]
pub(crate) struct Ansi16Writer<W> {
    inner: W,
    rewrite_colors: bool,
    pending: Vec<u8>,
}

#[cfg(any(not(windows), test))]
impl<W> Ansi16Writer<W> {
    fn new(inner: W, rewrite_colors: bool) -> Self {
        Self {
            inner,
            rewrite_colors,
            pending: Vec::new(),
        }
    }
}

#[cfg(any(not(windows), test))]
impl<W: Write> Ansi16Writer<W> {
    fn drain_complete_sequences(&mut self) -> io::Result<()> {
        let mut consumed = 0;
        while consumed < self.pending.len() {
            let Some(relative_escape) = self.pending[consumed..]
                .iter()
                .position(|byte| *byte == b'\x1b')
            else {
                self.inner.write_all(&self.pending[consumed..])?;
                consumed = self.pending.len();
                break;
            };
            let escape = consumed + relative_escape;
            self.inner.write_all(&self.pending[consumed..escape])?;
            if self.pending.get(escape + 1) != Some(&b'[') {
                if escape + 1 >= self.pending.len() {
                    consumed = escape;
                    break;
                }
                self.inner.write_all(&self.pending[escape..escape + 1])?;
                consumed = escape + 1;
                continue;
            }
            let Some(relative_final) = self.pending[escape + 2..]
                .iter()
                .position(|byte| (0x40..=0x7e).contains(byte))
            else {
                consumed = escape;
                break;
            };
            let final_index = escape + 2 + relative_final;
            let sequence = &self.pending[escape..=final_index];
            if sequence.last() == Some(&b'm') {
                self.inner.write_all(&rewrite_sgr_to_ansi16(sequence))?;
            } else {
                self.inner.write_all(sequence)?;
            }
            consumed = final_index + 1;
        }
        self.pending.drain(..consumed);
        Ok(())
    }
}

#[cfg(any(not(windows), test))]
impl<W: Write> Write for Ansi16Writer<W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if !self.rewrite_colors {
            return self.inner.write(buffer);
        }
        self.pending.extend_from_slice(buffer);
        self.drain_complete_sequences()?;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.pending.is_empty() {
            self.inner.write_all(&self.pending)?;
            self.pending.clear();
        }
        self.inner.flush()
    }
}

#[cfg(any(not(windows), test))]
fn rewrite_sgr_to_ansi16(sequence: &[u8]) -> Vec<u8> {
    let Ok(parameters) = std::str::from_utf8(&sequence[2..sequence.len() - 1]) else {
        return sequence.to_vec();
    };
    let fields = parameters.split(';').collect::<Vec<_>>();
    let mut rewritten = Vec::with_capacity(fields.len());
    let mut index = 0;
    while index < fields.len() {
        let is_foreground = fields[index] == "38";
        let is_background = fields[index] == "48";
        if (is_foreground || is_background)
            && fields.get(index + 1) == Some(&"5")
            && let Some(value) = fields
                .get(index + 2)
                .and_then(|field| field.parse::<u8>().ok())
            && value < 16
        {
            let base = if is_foreground {
                if value < 8 { 30 } else { 90 }
            } else if value < 8 {
                40
            } else {
                100
            };
            rewritten.push((base + value % 8).to_string());
            index += 3;
            continue;
        }
        rewritten.push(fields[index].to_owned());
        index += 1;
    }
    format!("\x1b[{}m", rewritten.join(";")).into_bytes()
}

trait LifecycleOps {
    fn initialize(&mut self) -> io::Result<()>;
    fn enable_bracketed_paste(&mut self) -> io::Result<()>;
    fn disable_bracketed_paste(&mut self) -> io::Result<()>;
    fn enable_mouse_capture(&mut self) -> io::Result<()>;
    fn disable_mouse_capture(&mut self) -> io::Result<()>;
    fn show_cursor(&mut self) -> io::Result<()>;
    fn restore(&mut self) -> io::Result<()>;
}

struct Lifecycle<O: LifecycleOps> {
    ops: O,
    initialized: bool,
    bracketed_paste: bool,
    mouse_capture: bool,
    restored: bool,
}

impl<O: LifecycleOps> Lifecycle<O> {
    fn enter(mut ops: O) -> io::Result<Self> {
        ops.initialize()?;
        let mut lifecycle = Self {
            ops,
            initialized: true,
            bracketed_paste: false,
            mouse_capture: false,
            restored: false,
        };
        if let Err(error) = lifecycle.ops.enable_bracketed_paste() {
            lifecycle.restore_best_effort();
            return Err(error);
        }
        lifecycle.bracketed_paste = true;
        if let Err(error) = lifecycle.ops.enable_mouse_capture() {
            lifecycle.restore_best_effort();
            return Err(error);
        }
        lifecycle.mouse_capture = true;
        Ok(lifecycle)
    }

    fn restore_best_effort(&mut self) {
        if self.restored {
            return;
        }
        if self.mouse_capture {
            let _ = self.ops.disable_mouse_capture();
            self.mouse_capture = false;
        }
        if self.bracketed_paste {
            let _ = self.ops.disable_bracketed_paste();
            self.bracketed_paste = false;
        }
        let _ = self.ops.show_cursor();
        if self.initialized {
            let _ = self.ops.restore();
            self.initialized = false;
        }
        self.restored = true;
    }
}

impl<O: LifecycleOps> Drop for Lifecycle<O> {
    fn drop(&mut self) {
        self.restore_best_effort();
    }
}

struct RealOps {
    terminal: Option<AppTerminal>,
    raw_mode: bool,
    alternate_screen: bool,
}

impl RealOps {
    const fn new() -> Self {
        Self {
            terminal: None,
            raw_mode: false,
            alternate_screen: false,
        }
    }
}

impl LifecycleOps for RealOps {
    fn initialize(&mut self) -> io::Result<()> {
        self.raw_mode = true;
        if let Err(error) = enable_raw_mode() {
            let _ = self.restore();
            return Err(error);
        }
        self.alternate_screen = true;
        if let Err(error) = execute!(io::stdout(), EnterAlternateScreen) {
            let _ = self.restore();
            return Err(error);
        }
        #[cfg(windows)]
        let backend = CrosstermBackend::new(io::stdout());
        #[cfg(not(windows))]
        let backend = CrosstermBackend::new(Ansi16Writer::new(
            io::stdout(),
            autoharness_tui::ColorDepth::detect() == autoharness_tui::ColorDepth::Basic16,
        ));
        match Terminal::new(backend) {
            Ok(terminal) => {
                self.terminal = Some(terminal);
                Ok(())
            }
            Err(error) => {
                let _ = self.restore();
                Err(error)
            }
        }
    }

    fn enable_bracketed_paste(&mut self) -> io::Result<()> {
        execute!(io::stdout(), EnableBracketedPaste)
    }

    fn disable_bracketed_paste(&mut self) -> io::Result<()> {
        execute!(io::stdout(), DisableBracketedPaste)
    }
    fn enable_mouse_capture(&mut self) -> io::Result<()> {
        execute!(io::stdout(), EnableMouseCapture)
    }

    fn disable_mouse_capture(&mut self) -> io::Result<()> {
        execute!(io::stdout(), DisableMouseCapture)
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        execute!(io::stdout(), Show)
    }

    fn restore(&mut self) -> io::Result<()> {
        self.terminal = None;
        let mut first_error = None;
        if self.alternate_screen {
            if let Err(error) = execute!(io::stdout(), LeaveAlternateScreen) {
                first_error = Some(error);
            }
            self.alternate_screen = false;
        }
        if self.raw_mode {
            if let Err(error) = disable_raw_mode()
                && first_error.is_none()
            {
                first_error = Some(error);
            }
            self.raw_mode = false;
        }
        first_error.map_or(Ok(()), Err)
    }
}

/// Owns terminal initialization and idempotent reverse-order restoration.
pub struct TerminalGuard {
    lifecycle: Lifecycle<RealOps>,
}

impl TerminalGuard {
    /// Enters raw mode and the alternate screen, then enables bracketed paste.
    pub fn enter() -> Result<Self, AppError> {
        let lifecycle = Lifecycle::enter(RealOps::new()).map_err(|_| AppError::Terminal)?;
        install_panic_restoration();
        Ok(Self { lifecycle })
    }

    /// Returns the Ratatui terminal owned by the guard.
    pub fn terminal_mut(&mut self) -> &mut AppTerminal {
        self.lifecycle
            .ops
            .terminal
            .as_mut()
            .expect("initialized terminal remains available until restoration")
    }

    /// Restores the terminal before application workers are awaited.
    pub fn restore(&mut self) {
        self.lifecycle.restore_best_effort();
    }
}

fn install_panic_restoration() {
    let prior = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |information| {
        restore_process_terminal();
        prior(information);
    }));
}

fn restore_process_terminal() {
    let _ = execute!(io::stdout(), DisableMouseCapture);
    let _ = execute!(io::stdout(), DisableBracketedPaste);
    let _ = execute!(io::stdout(), Show);
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
    let _ = disable_raw_mode();
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use super::*;

    #[test]
    fn ansi16_writer_rewrites_split_named_color_sequences() {
        let mut writer = Ansi16Writer::new(Vec::new(), true);

        writer.write_all(b"prefix\x1b[38;").expect("first fragment");
        writer
            .write_all(b"5;9;48;5;4mtext\x1b[39m")
            .expect("second fragment");
        writer.flush().expect("flush output");

        assert_eq!(writer.inner, b"prefix\x1b[91;44mtext\x1b[39m");
    }

    #[test]
    fn ansi16_writer_preserves_extended_colors_outside_the_system_palette() {
        let mut writer = Ansi16Writer::new(Vec::new(), true);

        writer
            .write_all(b"\x1b[38;5;45mindexed\x1b[48;2;1;2;3m")
            .expect("write output");
        writer.flush().expect("flush output");

        assert_eq!(writer.inner, b"\x1b[38;5;45mindexed\x1b[48;2;1;2;3m");
    }

    #[derive(Clone, Default)]
    struct FakeOps {
        calls: Arc<Mutex<Vec<&'static str>>>,
        fail_enable: bool,
        fail_mouse: bool,
    }

    impl FakeOps {
        fn record(&self, call: &'static str) {
            self.calls.lock().expect("call recorder").push(call);
        }
    }

    impl LifecycleOps for FakeOps {
        fn initialize(&mut self) -> io::Result<()> {
            self.record("initialize");
            Ok(())
        }

        fn enable_bracketed_paste(&mut self) -> io::Result<()> {
            self.record("enable_paste");
            if self.fail_enable {
                Err(io::Error::other("fixture failure"))
            } else {
                Ok(())
            }
        }

        fn disable_bracketed_paste(&mut self) -> io::Result<()> {
            self.record("disable_paste");
            Ok(())
        }

        fn enable_mouse_capture(&mut self) -> io::Result<()> {
            self.record("enable_mouse");
            if self.fail_mouse {
                Err(io::Error::other("fixture failure"))
            } else {
                Ok(())
            }
        }

        fn disable_mouse_capture(&mut self) -> io::Result<()> {
            self.record("disable_mouse");
            Ok(())
        }

        fn show_cursor(&mut self) -> io::Result<()> {
            self.record("show_cursor");
            Ok(())
        }

        fn restore(&mut self) -> io::Result<()> {
            self.record("restore");
            Ok(())
        }
    }

    #[test]
    fn normal_exit_restores_in_reverse_setup_order() {
        let mut lifecycle = Lifecycle::enter(FakeOps::default()).expect("fixture setup");

        lifecycle.restore_best_effort();
        lifecycle.restore_best_effort();

        assert_eq!(
            *lifecycle.ops.calls.lock().expect("call recorder"),
            vec![
                "initialize",
                "enable_paste",
                "enable_mouse",
                "disable_mouse",
                "disable_paste",
                "show_cursor",
                "restore",
            ]
        );
    }

    #[test]
    fn partial_entry_failure_still_shows_cursor_and_restores_terminal() {
        let operations = FakeOps {
            fail_enable: true,
            ..FakeOps::default()
        };
        let calls = Arc::clone(&operations.calls);

        let error = Lifecycle::enter(operations);

        assert!(error.is_err());
        assert_eq!(
            *calls.lock().expect("call recorder"),
            vec!["initialize", "enable_paste", "show_cursor", "restore",]
        );
    }

    #[test]
    fn mouse_capture_failure_restores_every_enabled_terminal_mode() {
        let operations = FakeOps {
            fail_mouse: true,
            ..FakeOps::default()
        };
        let calls = Arc::clone(&operations.calls);

        assert!(Lifecycle::enter(operations).is_err());
        assert_eq!(
            *calls.lock().expect("call recorder"),
            vec![
                "initialize",
                "enable_paste",
                "enable_mouse",
                "disable_paste",
                "show_cursor",
                "restore",
            ]
        );
    }
    #[test]
    fn drop_is_a_restoration_fallback() {
        let operations = FakeOps::default();
        let calls = Arc::clone(&operations.calls);
        let lifecycle = Lifecycle::enter(operations).expect("fixture setup");
        drop(lifecycle);

        assert_eq!(
            *calls.lock().expect("call recorder"),
            vec![
                "initialize",
                "enable_paste",
                "enable_mouse",
                "disable_mouse",
                "disable_paste",
                "show_cursor",
                "restore",
            ]
        );
    }
}
