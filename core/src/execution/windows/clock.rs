use std::time::Duration;

use super::super::Access;
use super::{MemoryError, Process32, Register32, guest};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockError {
    WentBackwards,
    OutOfRange,
}

#[derive(Clone, Copy)]
pub(super) enum Call {
    Frequency,
    Counter,
    Milliseconds,
}

impl Call {
    pub(super) fn arguments(self) -> usize {
        usize::from(!matches!(self, Self::Milliseconds))
    }

    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x220 => Some(Self::Frequency),
            0x224 => Some(Self::Counter),
            0x244 => Some(Self::Milliseconds),
            _ => None,
        }
    }
}

impl Process32 {
    /// supplies elapsed time from a stable, host-chosen session origin.
    /// the clock starts at zero; running guest work does not advance it.
    /// nanoseconds are counter units, not a timer accuracy guarantee.
    ///
    /// # errors
    /// rejects decreasing samples and values above `i64::MAX` nanoseconds without
    /// changing the clock, cpu or memory. equal samples are accepted.
    #[expect(clippy::missing_errors_doc, reason = "project headings are lower case")]
    pub fn set_elapsed_time(&mut self, elapsed: Duration) -> Result<(), ClockError> {
        let nanos = i64::try_from(elapsed.as_nanos()).map_err(|_| ClockError::OutOfRange)?;
        if nanos < self.elapsed_nanoseconds {
            return Err(ClockError::WentBackwards);
        }
        self.elapsed_nanoseconds = nanos;
        Ok(())
    }

    pub(super) fn query_clock(&mut self, call: Call, output: u32) -> Result<(), MemoryError> {
        let value = match call {
            Call::Frequency => 1_000_000_000,
            Call::Counter => self.elapsed_nanoseconds,
            Call::Milliseconds => {
                self.cpu
                    .set_register(Register32::Eax, self.elapsed_milliseconds());
                return Ok(());
            }
        };
        guest::check(&self.memory, output, 8, Access::Write)?;
        self.memory.write(u64::from(output), &value.to_le_bytes())?;
        self.cpu.set_register(Register32::Eax, 1);
        Ok(())
    }

    pub(super) fn elapsed_milliseconds(&self) -> u32 {
        let millis = (self.elapsed_nanoseconds / 1_000_000) & i64::from(u32::MAX);
        u32::try_from(millis).expect("masked millisecond counter")
    }
}
