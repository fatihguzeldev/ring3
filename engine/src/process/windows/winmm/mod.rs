mod calls;
mod timers;
pub(in crate::process) use timers::WinmmThreadReply32;
#[cfg(test)]
mod tests;
use crate::windows::winmm::TimerPeriods32;
#[derive(Default)]
pub(in crate::process) struct WinmmRuntime32 {
    pub(in crate::process::windows::winmm) periods: TimerPeriods32,
    timers: crate::windows::winmm::timers::MultimediaTimerState32,
    reply: Option<timers::TimerReply32>,
    captures: Vec<(u32, u32)>,
}

impl WinmmRuntime32 {
    pub(in crate::process) fn terminate_process(&mut self) {
        self.periods.clear();
        self.timers.clear();
        self.reply = None;
        self.captures.clear();
    }
}
