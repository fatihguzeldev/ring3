use crate::windows::user32::{Message32, User32Error32};

#[derive(Clone)]
pub(in crate::process::windows::user32) struct Timer32 {
    pub thread: u32,
    pub window: u32,
    pub id: u32,
    pub procedure: u32,
    pub activation: crate::windows::activation::CapturedActivation32,
    interval: u64,
    deadline: u64,
    due: bool,
    fresh: bool,
}

#[derive(Clone)]
struct TimerMessage32 {
    message: Message32,
    sequence: u64,
    activation: crate::windows::activation::CapturedActivation32,
}

pub(in crate::process::windows::user32) struct TimerSet32 {
    pub thread: u32,
    pub window: u32,
    pub id: u32,
    pub delay: u32,
    pub now: u64,
    pub procedure: u32,
    pub activation: crate::windows::activation::CapturedActivation32,
}

pub(in crate::process::windows::user32) struct TimerState32 {
    pub(in crate::process::windows::user32) timers: Vec<Timer32>,
    pending: Vec<TimerMessage32>,
    next_message: u64,
    pub(in crate::process::windows::user32) clock_rows: Vec<(u32, u32, u32, u64)>,
    limit: usize,
    pub(in crate::process::windows::user32) window_limit: usize,
    next_id: u32,
    pub(in crate::process::windows::user32) now: u64,
    pub(in crate::process::windows::user32) tick: u32,
}

impl TimerState32 {
    pub(in crate::process::windows::user32) fn new(
        limit: usize,
        window_limit: usize,
    ) -> Result<Self, User32Error32> {
        let mut timers = Vec::new();
        let mut pending = Vec::new();
        let mut clock_rows = Vec::new();
        clock_rows
            .try_reserve_exact(limit)
            .map_err(|_| User32Error32::Allocation)?;
        timers
            .try_reserve_exact(limit)
            .map_err(|_| User32Error32::Allocation)?;
        pending
            .try_reserve_exact(limit)
            .map_err(|_| User32Error32::Allocation)?;
        Ok(Self {
            timers,
            pending,
            next_message: 0,
            clock_rows,
            limit,
            window_limit,
            next_id: 0,
            now: 0,
            tick: 0,
        })
    }
    pub(in crate::process::windows::user32) fn set(
        &mut self,
        registration: TimerSet32,
    ) -> Result<u32, User32Error32> {
        let TimerSet32 {
            thread,
            window,
            id,
            delay,
            now,
            procedure,
            activation,
        } = registration;
        let interval = u64::from(delay.clamp(10, 0x7fff_ffff)) * 1_000_000;
        let deadline = self
            .now
            .max(now)
            .checked_add(interval)
            .ok_or(User32Error32::Capacity)?;
        let found = self
            .timers
            .iter()
            .position(|timer| timer.thread == thread && timer.window == window && timer.id == id);
        if found.is_none() && self.timers.len() == self.limit {
            return Err(User32Error32::Win32(1450));
        }
        let id = if window == 0 && found.is_none() {
            let mut next = self.next_id;
            for _ in 0..=self.timers.len() {
                next = next.wrapping_add(1).max(1);
                if !self
                    .timers
                    .iter()
                    .any(|timer| timer.window == 0 && timer.id == next)
                {
                    break;
                }
            }
            self.next_id = next;
            next
        } else {
            id
        };
        let timer = Timer32 {
            thread,
            window,
            id,
            procedure,
            activation,
            interval,
            deadline,
            due: false,
            fresh: false,
        };
        if let Some(index) = found {
            self.timers[index] = timer;
        } else {
            self.timers.push(timer);
        }
        Ok(id.max(1))
    }
    pub(in crate::process::windows::user32) fn kill(
        &mut self,
        thread: u32,
        window: u32,
        id: u32,
    ) -> bool {
        let Some(index) = self
            .timers
            .iter()
            .position(|timer| timer.thread == thread && timer.window == window && timer.id == id)
        else {
            return false;
        };
        self.timers.swap_remove(index);
        true
    }
    pub(in crate::process::windows::user32) fn check_clock(
        &self,
        now: u64,
    ) -> Result<(), User32Error32> {
        if now < self.now {
            Err(User32Error32::InvalidRequest)
        } else {
            Ok(())
        }
    }
    pub(in crate::process::windows::user32) fn observe_clock(&mut self, now: u64, tick: u32) {
        assert!(now >= self.now);
        self.now = now;
        self.tick = tick;
        for timer in &mut self.timers {
            if !timer.due && timer.deadline <= now {
                timer.due = true;
                timer.fresh = true;
            }
        }
    }
    pub(in crate::process::windows::user32) fn deadline(&self) -> Option<u64> {
        self.timers
            .iter()
            .filter(|timer| !timer.due)
            .map(|timer| timer.deadline)
            .min()
    }
    pub(in crate::process::windows::user32) fn status_at(
        &self,
        thread: u32,
        now: u64,
    ) -> (u32, u32) {
        let pending = self
            .pending
            .iter()
            .any(|message| message.message.thread_id == thread);
        let due = self
            .timers
            .iter()
            .any(|timer| timer.thread == thread && (timer.due || timer.deadline <= now));
        let fresh = self.timers.iter().any(|timer| {
            timer.thread == thread && (timer.fresh || !timer.due && timer.deadline <= now)
        });
        (u32::from(pending || due) * 0x10, u32::from(fresh) * 0x10)
    }
    pub(in crate::process::windows::user32) fn observe(&mut self, thread: u32, mask: u32) {
        if mask & 0x10 != 0 {
            for timer in self
                .timers
                .iter_mut()
                .filter(|timer| timer.thread == thread)
            {
                timer.fresh = false;
            }
        }
    }
    pub(in crate::process::windows::user32) fn pending(
        &self,
        matches: impl Fn(Message32) -> bool,
    ) -> Option<(usize, Message32)> {
        self.pending
            .iter()
            .enumerate()
            .find(|(_, message)| matches(message.message))
            .map(|(index, message)| (index, message.message))
    }
    pub(in crate::process::windows::user32) fn pending_activation(
        &self,
        index: usize,
    ) -> (u64, crate::windows::activation::CapturedActivation32) {
        let message = &self.pending[index];
        (message.sequence, message.activation.clone())
    }
    pub(in crate::process::windows::user32) fn next_message_identity(&self) -> u64 {
        self.next_message + 1
    }
    pub(in crate::process::windows::user32) fn remove_pending(&mut self, index: usize) {
        self.pending.remove(index);
    }
    pub(in crate::process::windows::user32) fn due(
        &self,
        matches: impl Fn(&Timer32) -> bool,
    ) -> Option<(usize, Timer32)> {
        self.timers.iter().cloned().enumerate().find(|(_, timer)| {
            timer.due
                && matches(timer)
                && !self.pending.iter().any(|message| {
                    message.message.thread_id == timer.thread
                        && message.message.window == timer.window
                        && message.message.wparam == timer.id
                })
        })
    }
    pub(in crate::process::windows::user32) fn prepare_message(
        &self,
        index: usize,
        retain: bool,
    ) -> Result<u64, User32Error32> {
        if retain && self.pending.len() == self.limit {
            return Err(User32Error32::Win32(1816));
        }
        self.next_message
            .checked_add(1)
            .ok_or(User32Error32::Capacity)?;
        let timer = &self.timers[index];
        let periods = (self.now - timer.deadline) / timer.interval + 1;
        timer
            .deadline
            .checked_add(
                periods
                    .checked_mul(timer.interval)
                    .ok_or(User32Error32::Capacity)?,
            )
            .ok_or(User32Error32::Capacity)
    }
    pub(in crate::process::windows::user32) fn commit_message(
        &mut self,
        index: usize,
        deadline: u64,
        retained: Option<Message32>,
    ) {
        let timer = &mut self.timers[index];
        self.next_message += 1;
        timer.deadline = deadline;
        timer.due = false;
        timer.fresh = false;
        if let Some(message) = retained {
            self.pending.push(TimerMessage32 {
                message,
                sequence: self.next_message,
                activation: timer.activation.clone(),
            });
        }
    }
    pub(in crate::process::windows::user32) fn retire_window(&mut self, window: u32) {
        self.timers.retain(|timer| timer.window != window);
        self.pending
            .retain(|message| message.message.window != window);
    }
    pub(in crate::process::windows::user32) fn retire_thread(&mut self, thread: u32) {
        self.timers.retain(|timer| timer.thread != thread);
        self.pending
            .retain(|message| message.message.thread_id != thread);
    }
    pub(in crate::process::windows::user32) fn clear(&mut self) {
        self.timers.clear();
        self.pending.clear();
    }
}
