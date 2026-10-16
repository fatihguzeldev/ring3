mod calls;
#[cfg(test)]
mod tests;
use crate::windows::winmm::TimerPeriods32;
#[derive(Default)]
pub(in crate::process) struct WinmmRuntime32 {
    pub(in crate::process::windows::winmm) periods: TimerPeriods32,
}

impl WinmmRuntime32 {
    pub(in crate::process) fn terminate_process(&mut self) {
        self.periods.clear();
    }
}
