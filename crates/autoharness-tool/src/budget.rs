use std::time::{Duration, Instant};

use autoharness_domain::RunLimits;

use crate::{ToolError, ToolErrorKind};
use autoharness_domain::RetryAdvice;

/// Process-local counters checked against immutable durable run limits.
#[derive(Clone, Debug)]
pub struct RunBudget {
    limits: RunLimits,
    started: Instant,
    elapsed_before_start: Duration,
    turns: u32,
    tokens: u64,
    output_bytes: u64,
    active_tools: u32,
}

impl RunBudget {
    /// Starts a budget from immutable limits.
    #[must_use]
    pub fn new(limits: RunLimits) -> Self {
        Self {
            limits,
            started: Instant::now(),
            elapsed_before_start: Duration::ZERO,
            turns: 0,
            tokens: 0,
            output_bytes: 0,
            active_tools: 0,
        }
    }

    /// Returns immutable limits for durable admission.
    #[must_use]
    pub const fn limits(&self) -> RunLimits {
        self.limits
    }

    /// Reconstructs durable counters while preserving elapsed wall time across a restart.
    pub fn restore(
        limits: RunLimits,
        elapsed: Duration,
        turns: u32,
        tokens: u64,
        output_bytes: u64,
        active_tools: u32,
    ) -> Result<Self, ToolError> {
        if turns > limits.max_turns {
            return Err(limit(ToolErrorKind::TurnLimit));
        }
        if tokens > limits.max_tokens
            || output_bytes > limits.max_output_bytes
            || active_tools > limits.max_concurrency
        {
            return Err(limit(ToolErrorKind::OutputLimit));
        }
        Ok(Self {
            limits,
            started: Instant::now(),
            elapsed_before_start: elapsed,
            turns,
            tokens,
            output_bytes,
            active_tools,
        })
    }

    /// Checks elapsed time without changing counters.
    pub fn check_time(&self) -> Result<(), ToolError> {
        if self
            .elapsed_before_start
            .saturating_add(self.started.elapsed())
            > Duration::from_millis(self.limits.max_time_ms)
        {
            Err(limit(ToolErrorKind::Timeout))
        } else {
            Ok(())
        }
    }

    /// Admits one provider turn.
    pub fn start_turn(&mut self) -> Result<u32, ToolError> {
        self.check_time()?;
        let next = self.turns.saturating_add(1);
        if next > self.limits.max_turns {
            return Err(limit(ToolErrorKind::TurnLimit));
        }
        self.turns = next;
        Ok(next)
    }

    /// Replaces cumulative provider token usage.
    pub fn record_tokens(&mut self, total: u64) -> Result<(), ToolError> {
        self.check_time()?;
        if total > self.limits.max_tokens || total < self.tokens {
            return Err(limit(ToolErrorKind::OutputLimit));
        }
        self.tokens = total;
        Ok(())
    }

    /// Adds provider or tool output bytes.
    pub fn add_output(&mut self, bytes: u64) -> Result<(), ToolError> {
        self.check_time()?;
        let next = self
            .output_bytes
            .checked_add(bytes)
            .ok_or_else(|| limit(ToolErrorKind::OutputLimit))?;
        if next > self.limits.max_output_bytes {
            return Err(limit(ToolErrorKind::OutputLimit));
        }
        self.output_bytes = next;
        Ok(())
    }

    /// Admits one concurrent tool execution.
    pub fn start_tool(&mut self) -> Result<(), ToolError> {
        self.check_time()?;
        let next = self.active_tools.saturating_add(1);
        if next > self.limits.max_concurrency {
            return Err(limit(ToolErrorKind::OutputLimit));
        }
        self.active_tools = next;
        Ok(())
    }

    /// Releases one concurrent tool slot.
    pub fn finish_tool(&mut self) {
        self.active_tools = self.active_tools.saturating_sub(1);
    }
}

fn limit(kind: ToolErrorKind) -> ToolError {
    ToolError::new(kind, RetryAdvice::Never)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> RunLimits {
        RunLimits::new(1, 60_000, 10, 30, 1).expect("limits")
    }

    #[test]
    fn every_counter_fails_closed_at_its_bound() {
        let mut budget = RunBudget::new(limits());
        assert!(budget.start_turn().is_ok());
        assert!(budget.start_turn().is_err());
        assert!(budget.record_tokens(11).is_err());
        assert!(budget.add_output(31).is_err());
        assert!(budget.start_tool().is_ok());
        assert!(budget.start_tool().is_err());
    }

    #[test]
    fn restored_elapsed_time_does_not_reset_the_deadline() {
        let budget = RunBudget::restore(limits(), Duration::from_millis(60_001), 1, 10, 30, 0)
            .expect("durable counters fit");
        assert!(budget.check_time().is_err());
    }
}
