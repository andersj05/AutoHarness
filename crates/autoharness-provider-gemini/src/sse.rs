use autoharness_domain::RetryAdvice;
use autoharness_provider::{ProviderError, ProviderErrorKind};

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct SseFrame {
    pub(crate) event: Option<String>,
    pub(crate) data: String,
}

pub(crate) struct SseDecoder {
    buffer: Vec<u8>,
    max_frame_bytes: usize,
    first_frame: bool,
    line_has_content: bool,
    pending_carriage_return: bool,
}

impl SseDecoder {
    pub(crate) fn new(max_frame_bytes: usize) -> Self {
        Self {
            buffer: Vec::new(),
            max_frame_bytes,
            first_frame: true,
            line_has_content: false,
            pending_carriage_return: false,
        }
    }

    pub(crate) fn push(&mut self, bytes: &[u8]) -> Result<Vec<SseFrame>, ProviderError> {
        let mut frames = Vec::new();
        for byte in bytes {
            if *byte == b'\n' && self.pending_carriage_return {
                self.pending_carriage_return = false;
                // A CR that completed a blank line already drained its frame.
                if self.buffer.is_empty() {
                    continue;
                }
                if self.buffer.len() == self.max_frame_bytes {
                    return Err(limit_error());
                }
                self.buffer.push(*byte);
                continue;
            }
            self.pending_carriage_return = false;
            if self.buffer.len() == self.max_frame_bytes {
                return Err(limit_error());
            }
            self.buffer.push(*byte);

            match *byte {
                b'\r' | b'\n' => {
                    let blank_line = !self.line_has_content;
                    self.line_has_content = false;
                    self.pending_carriage_return = *byte == b'\r';
                    if blank_line {
                        let event = std::mem::take(&mut self.buffer);
                        if let Some(frame) = self.decode_frame(&event)? {
                            frames.push(frame);
                        }
                    }
                }
                _ => self.line_has_content = true,
            }
        }
        Ok(frames)
    }

    pub(crate) fn finish(&mut self) -> Result<(), ProviderError> {
        if self.buffer.iter().all(u8::is_ascii_whitespace) {
            self.buffer.clear();
            return Ok(());
        }
        Err(protocol_error())
    }

    fn decode_frame(&mut self, bytes: &[u8]) -> Result<Option<SseFrame>, ProviderError> {
        let mut text = std::str::from_utf8(bytes).map_err(|_| protocol_error())?;
        if self.first_frame {
            self.first_frame = false;
            text = text.strip_prefix('\u{feff}').unwrap_or(text);
        }

        let mut event = None;
        let mut data_lines = Vec::new();
        for raw_line in text.split(['\r', '\n']) {
            if raw_line.is_empty() || raw_line.starts_with(':') {
                continue;
            }
            let (field, value) = raw_line
                .split_once(':')
                .map_or((raw_line, ""), |(field, value)| {
                    (field, value.strip_prefix(' ').unwrap_or(value))
                });
            match field {
                "event" => event = Some(value.to_owned()),
                "data" => data_lines.push(value),
                _ => {}
            }
        }
        if data_lines.is_empty() {
            return Ok(None);
        }
        Ok(Some(SseFrame {
            event,
            data: data_lines.join("\n"),
        }))
    }
}

fn protocol_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::Protocol, RetryAdvice::Never)
}

fn limit_error() -> ProviderError {
    ProviderError::new(ProviderErrorKind::LimitExceeded, RetryAdvice::Never)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_crlf_lf_multiline_data_and_comments() {
        let mut decoder = SseDecoder::new(1024);
        let frames = decoder
            .push(b": heartbeat\r\nevent: step.delta\r\ndata: {\"a\":\r\ndata: 1}\r\n\r\n")
            .expect("valid SSE");

        assert_eq!(
            frames,
            vec![SseFrame {
                event: Some("step.delta".to_owned()),
                data: "{\"a\":\n1}".to_owned(),
            }]
        );
    }

    #[test]
    fn accepts_every_two_way_byte_fragmentation() {
        let input = "event: step.delta\r\ndata: {\"text\":\"é🙂\"}\r\n\r\n".as_bytes();
        for split in 0..=input.len() {
            let mut decoder = SseDecoder::new(1024);
            let mut frames = decoder.push(&input[..split]).expect("first fragment");
            frames.extend(decoder.push(&input[split..]).expect("second fragment"));
            assert_eq!(frames.len(), 1, "split at byte {split}");
            assert_eq!(frames[0].event.as_deref(), Some("step.delta"));
        }
    }

    #[test]
    fn rejects_truncated_or_oversized_frames() {
        let mut truncated = SseDecoder::new(1024);
        truncated.push(b"data: {\"x\":1}").expect("buffer fragment");
        assert_eq!(
            truncated
                .finish()
                .expect_err("must reject truncation")
                .kind(),
            ProviderErrorKind::Protocol
        );

        let mut oversized = SseDecoder::new(8);
        assert_eq!(
            oversized
                .push(b"data: too large")
                .expect_err("must enforce bound")
                .kind(),
            ProviderErrorKind::LimitExceeded
        );
    }
}
