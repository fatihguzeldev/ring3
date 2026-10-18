use std::collections::VecDeque;

use super::{Show32, ShowStage32};
use crate::{
    memory::AddressSpace,
    process::windows::user32::{
        User32CallContext32, User32Callback32, User32Context32, User32Outcome32,
        callback::Continuation32,
    },
    windows::{
        user32::{PreparedSentBatch32, User32Api32, User32Error32},
        window::WindowHandle32,
    },
};

pub(in crate::process::windows::user32) struct AppActivation32 {
    active_thread: Option<u32>,
    notifications: VecDeque<AppNotification32>,
    limit: usize,
}

#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct AppNotification32 {
    window: WindowHandle32,
    thread: u32,
    active: bool,
    other_thread: u32,
}

pub(in crate::process::windows::user32) struct PreparedAppActivation32 {
    active_thread: Option<u32>,
    notifications: Vec<AppNotification32>,
    sent: PreparedSentBatch32,
}

impl PreparedAppActivation32 {
    pub(in crate::process::windows::user32) fn queue_rows(
        &self,
    ) -> impl Iterator<Item = (u32, u32, u32, u64)> + '_ {
        self.sent.rows()
    }
}

#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) enum AppResume32 {
    Show(Show32),
    Retrieve {
        api: User32Api32,
        arguments: [u32; 5],
        context: User32CallContext32,
    },
}

impl AppResume32 {
    fn thread(self) -> u32 {
        match self {
            Self::Show(show) => show.thread,
            Self::Retrieve { context, .. } => context.thread_id,
        }
    }
}

impl AppActivation32 {
    pub(in crate::process::windows::user32) fn new(limit: usize) -> Result<Self, User32Error32> {
        let mut notifications = VecDeque::new();
        notifications
            .try_reserve_exact(limit)
            .map_err(|_| User32Error32::Allocation)?;
        Ok(Self {
            active_thread: None,
            notifications,
            limit,
        })
    }

    pub(in crate::process::windows::user32) fn pending(&self, thread: u32) -> bool {
        self.notifications
            .iter()
            .any(|notification| notification.thread == thread)
    }

    pub(in crate::process::windows::user32) fn retire_thread(&mut self, thread: u32) {
        self.notifications
            .retain(|notification| notification.thread != thread);
        if self.active_thread == Some(thread) {
            self.active_thread = None;
        }
    }
}

impl User32Context32 {
    pub(in crate::process) fn app_queue_threads(&self, current: u32) -> ([u32; 2], usize) {
        match self
            .app_activation
            .active_thread
            .filter(|&thread| thread != current)
        {
            Some(other) => ([current, other], 2),
            None => ([current, 0], 1),
        }
    }

    pub(in crate::process) fn app_queue_may_change(&self, token: u64) -> bool {
        self.pending.last().is_some_and(|pending| {
            pending.token == token
                && matches!(
                    pending.continuation,
                    Continuation32::Show(_)
                        | Continuation32::AppActivation { .. }
                        | Continuation32::Destroy { .. }
                        | Continuation32::Create { visible: true, .. }
                )
        })
    }

    pub(in crate::process::windows::user32) fn prepare_app_activation(
        &self,
        focused: bool,
        active: Option<WindowHandle32>,
    ) -> Result<Option<PreparedAppActivation32>, User32Error32> {
        let next = active
            .filter(|_| focused)
            .map(|window| {
                self.windows()
                    .get(window)
                    .map(|record| record.description.thread_id)
            })
            .transpose()?;
        let previous = self.app_activation.active_thread;
        if previous == next {
            return Ok(None);
        }
        let mut notifications = Vec::new();
        let mut changes = [(0, 0); 2];
        let mut count = 0;
        for (thread, active, other_thread) in [
            (previous, false, next.unwrap_or(0)),
            (next, true, previous.unwrap_or(0)),
        ] {
            let Some(thread) = thread else { continue };
            let start = notifications.len();
            for window in self.state.all_windows() {
                let record = self.windows().get(window)?;
                let data = self.state.record(window)?;
                if record.description.thread_id != thread
                    || data.style & 0x4000_0000 != 0
                    || data.destroying
                    || self.compatibility.desktop == Some(window)
                {
                    continue;
                }
                if self.app_activation.notifications.len() + notifications.len()
                    == self.app_activation.limit
                {
                    return Err(User32Error32::Capacity);
                }
                notifications
                    .try_reserve(1)
                    .map_err(|_| User32Error32::Allocation)?;
                notifications.push(AppNotification32 {
                    window,
                    thread,
                    active,
                    other_thread,
                });
            }
            if notifications.len() != start {
                changes[count] = (thread, (notifications.len() - start) as i32);
                count += 1;
            }
        }
        let sent = self.state.prepare_sent_batch(&changes[..count])?;
        Ok(Some(PreparedAppActivation32 {
            active_thread: next,
            notifications,
            sent,
        }))
    }

    pub(in crate::process::windows::user32) fn commit_app_activation(
        &mut self,
        prepared: PreparedAppActivation32,
    ) {
        self.state.commit_sent_batch(prepared.sent);
        self.app_activation.active_thread = prepared.active_thread;
        self.app_activation
            .notifications
            .extend(prepared.notifications);
    }

    pub(in crate::process::windows::user32) fn retire_window_app_activation(
        &mut self,
        window: WindowHandle32,
    ) -> Result<(), User32Error32> {
        let count = self
            .app_activation
            .notifications
            .iter()
            .filter(|notification| notification.window == window)
            .count();
        if count != 0 {
            let thread = self.windows().get(window)?.description.thread_id;
            let prepared = self.state.prepare_sent_change(thread, -(count as i32))?;
            self.state.commit_sent_change(prepared);
            self.app_activation
                .notifications
                .retain(|notification| notification.window != window);
        }
        let thread = self.windows().get(window)?.description.thread_id;
        if self.app_activation.active_thread == Some(thread)
            && !self.state.all_windows().any(|other| {
                other != window
                    && self
                        .windows()
                        .get(other)
                        .is_ok_and(|record| record.description.thread_id == thread)
                    && self
                        .state
                        .record(other)
                        .is_ok_and(|data| !data.destroying && data.style & 0x4000_0000 == 0)
                    && self.compatibility.desktop != Some(other)
            })
        {
            self.app_activation.active_thread = None;
        }
        Ok(())
    }

    fn take_app_notification(
        &mut self,
        thread: u32,
    ) -> Result<Option<AppNotification32>, User32Error32> {
        while let Some(index) = self
            .app_activation
            .notifications
            .iter()
            .position(|notification| notification.thread == thread)
        {
            let notification = self.app_activation.notifications[index];
            let sent = self.state.prepare_sent_change(thread, -1)?;
            self.state.commit_sent_change(sent);
            self.app_activation.notifications.remove(index);
            if self
                .windows()
                .get(notification.window)
                .is_ok_and(|record| record.description.thread_id == thread)
                && self
                    .state
                    .record(notification.window)
                    .is_ok_and(|data| !data.destroying)
            {
                return Ok(Some(notification));
            }
        }
        Ok(None)
    }

    pub(in crate::process::windows::user32) fn begin_app_notifications(
        &mut self,
        resume: AppResume32,
    ) -> Result<Option<User32Outcome32>, User32Error32> {
        if !self.app_activation.pending(resume.thread()) {
            return Ok(None);
        }
        if matches!(resume, AppResume32::Retrieve { .. }) {
            self.check_continuation()?;
        }
        let Some(notification) = self.take_app_notification(resume.thread())? else {
            return Ok(None);
        };
        let continuation = Continuation32::AppActivation {
            notification,
            resume,
        };
        if matches!(resume, AppResume32::Show(_)) {
            self.pending
                .last_mut()
                .ok_or(User32Error32::InvalidToken)?
                .continuation = continuation;
            self.next_outcome().map(Some)
        } else {
            self.start(continuation).map(Some)
        }
    }

    pub(in crate::process::windows::user32) fn app_notification_outcome(
        &self,
        notification: AppNotification32,
    ) -> Result<User32Outcome32, User32Error32> {
        let record = self.windows().get(notification.window)?;
        if record.description.thread_id != notification.thread {
            return Err(User32Error32::InvalidToken);
        }
        Ok(User32Outcome32::Callback(User32Callback32 {
            procedure: record.description.procedure,
            arguments: [
                notification.window.get(),
                0x1c,
                u32::from(notification.active),
                notification.other_thread,
            ],
            continuation: self
                .pending
                .last()
                .ok_or(User32Error32::InvalidToken)?
                .token,
            count: 4,
        }))
    }

    pub(in crate::process::windows::user32) fn complete_app_notifications(
        &mut self,
        resume: AppResume32,
        memory: &mut AddressSpace,
    ) -> Result<User32Outcome32, User32Error32> {
        if let Some(notification) = self.take_app_notification(resume.thread())? {
            self.pending.last_mut().unwrap().continuation = Continuation32::AppActivation {
                notification,
                resume,
            };
            return self.next_outcome();
        }
        match resume {
            AppResume32::Show(show) => self.resume_show_after_app(show),
            AppResume32::Retrieve {
                api,
                arguments,
                context,
            } => {
                let pending = self.pending.pop().ok_or(User32Error32::InvalidToken)?;
                let outcome = self.retrieve_message(
                    api,
                    &arguments[..api.stack_words() as usize],
                    memory,
                    context,
                );
                if outcome.is_err() {
                    self.pending.push(pending);
                }
                outcome
            }
        }
    }

    pub(super) fn resume_show_after_app(
        &mut self,
        show: Show32,
    ) -> Result<User32Outcome32, User32Error32> {
        if self.windows().active() != show.target || !self.show_target_valid(show) {
            return self.finish_show(show, false);
        }
        self.pending
            .last_mut()
            .ok_or(User32Error32::InvalidToken)?
            .continuation = Continuation32::Show(show);
        if matches!(show.stage, ShowStage32::Finish) {
            self.finish_show(show, true)
        } else {
            self.next_outcome()
        }
    }
}
