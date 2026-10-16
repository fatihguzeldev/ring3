use super::super::User32Context32;
use crate::process::{EngineInstance, HostError};

pub(in crate::process) struct PreparedUser32TimerClock32 {
    now: u64,
    previous: u64,
    tick: u32,
    configured: bool,
    rows: Vec<(u32, u32, u32, u64)>,
}
impl PreparedUser32TimerClock32 {
    pub(in crate::process) fn queue_rows(&self) -> &[(u32, u32, u32, u64)] {
        &self.rows
    }
}
impl User32Context32 {
    pub(in crate::process) fn timer_deadline(&self) -> Option<u64> {
        self.timers.deadline()
    }
    pub(in crate::process) fn queue_status_with_timers(
        &self,
        thread: u32,
        now: u64,
    ) -> (u32, u32, u64) {
        let (current, fresh, generation) = self.state.queue_status(thread);
        let timer = self.timers.status_at(thread, now);
        (current | timer.0, fresh | timer.1, generation)
    }
}
impl EngineInstance {
    pub(in crate::process) fn user32_timer_deadline(&self) -> Option<u64> {
        self.user32
            .as_ref()
            .and_then(User32Context32::timer_deadline)
    }
    pub(in crate::process) fn prepare_user32_timer_clock(
        &mut self,
        now: u64,
    ) -> Result<PreparedUser32TimerClock32, HostError> {
        let Some(context) = self.user32.as_mut() else {
            return Ok(PreparedUser32TimerClock32 {
                now,
                previous: 0,
                tick: 0,
                configured: false,
                rows: Vec::new(),
            });
        };
        context.timers.check_clock(now).map_err(HostError::User32)?;
        let (origin, _) = self.cpu_clock.monotonic_bounds()?;
        if now < origin {
            return Err(HostError::InvalidRequest);
        }
        let mut rows = std::mem::take(&mut context.timers.clock_rows);
        rows.clear();
        if rows.capacity() < context.timers.timers.len()
            && rows.try_reserve_exact(context.timers.timers.len()).is_err()
        {
            context.timers.clock_rows = rows;
            return Err(HostError::Infrastructure);
        }
        for timer in &context.timers.timers {
            if !rows.iter().any(|row| row.0 == timer.thread) {
                let (current, fresh, generation) =
                    context.queue_status_with_timers(timer.thread, now);
                rows.push((timer.thread, current, fresh, generation));
            }
        }
        Ok(PreparedUser32TimerClock32 {
            now,
            previous: context.timers.now,
            tick: ((now - origin) / 1_000_000) as u32,
            configured: true,
            rows,
        })
    }
    pub(in crate::process) fn commit_user32_timer_clock(
        &mut self,
        prepared: PreparedUser32TimerClock32,
    ) {
        if !prepared.configured {
            assert!(self.user32.is_none());
            return;
        }
        let context = self.user32.as_mut().unwrap();
        assert_eq!(context.timers.now, prepared.previous);
        context.timers.observe_clock(prepared.now, prepared.tick);
        context.timers.clock_rows = prepared.rows;
    }
}
