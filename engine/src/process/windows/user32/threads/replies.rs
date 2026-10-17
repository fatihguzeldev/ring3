use super::super::{User32Context32, callback::Pending32, destruction::DestroyPlan32};
use crate::{
    input::keyboard::KeyboardLayout32,
    windows::user32::{Message32, User32Error32},
};

struct ThreadInput32 {
    queued: [u8; 256],
    message: Message32,
    dead: Option<char>,
    extra_info: u32,
    layout: KeyboardLayout32,
    layout_revision: u64,
    show_count: i32,
}
pub(in crate::process) struct User32ThreadReply32 {
    image: Option<super::super::images::ImageReply32>,
    pending: Vec<Pending32>,
    destructions: Vec<DestroyPlan32>,
    last_error: Option<u32>,
    input: ThreadInput32,
}
impl User32Context32 {
    pub(in crate::process) fn thread_reply_ready(&self, reply: &User32ThreadReply32) -> bool {
        self.pending_wait_ready(reply.pending.last())
    }
    pub(in crate::process) fn active_thread_wait_ready(&self) -> bool {
        self.pending_wait_ready(self.pending.last())
    }
    fn pending_wait_ready(&self, pending: Option<&Pending32>) -> bool {
        let Some(pending) = pending else {
            return false;
        };
        match pending.continuation {
            super::super::callback::Continuation32::ResourceDialog { plan, .. } => {
                self.resource_dialog_ready(plan)
            }
            super::super::callback::Continuation32::Dialog { id } => self.dialog_ready(id),
            super::super::callback::Continuation32::MenuPopup { plan, .. } => {
                self.popup_ready(plan)
            }
            super::super::callback::Continuation32::RemoteCall { ready, .. }
            | super::super::callback::Continuation32::DialogRemoteHelp { ready, .. } => {
                ready.is_some()
            }
            super::super::callback::Continuation32::Clipboard(call) => !call.bytes_wait,
            super::super::callback::Continuation32::MenuBar(_) => true,
            super::super::callback::Continuation32::TooltipTrack(_) => true,
            super::super::callback::Continuation32::TooltipRelay(_) => true,
            super::super::callback::Continuation32::Numeric(_) => true,
            super::super::callback::Continuation32::TooltipText(_) => true,
            super::super::callback::Continuation32::TabTooltip(_) => true,
            super::super::callback::Continuation32::Header(_) => true,
            super::super::callback::Continuation32::ListView(_) => true,
            super::super::callback::Continuation32::TabSelection(_) => true,
            super::super::callback::Continuation32::StatusDraw(_) => true,
            super::super::callback::Continuation32::CommonLayout(call) => {
                call.child.is_none() && call.completed.is_some()
            }
            super::super::callback::Continuation32::FilePrompt(_)
            | super::super::callback::Continuation32::Retrieve { .. }
            | super::super::callback::Continuation32::Accelerator(_)
            | super::super::callback::Continuation32::SystemColors(_)
            | super::super::callback::Continuation32::UiState(_)
            | super::super::callback::Continuation32::SentPaint(_) => true,
            super::super::callback::Continuation32::MessageWait {
                thread,
                window,
                min,
                max,
                ..
            } => match self.state.find_message(thread, window, min, max) {
                Ok(message) => {
                    message.is_some()
                        || self
                            .state
                            .paint_message(
                                thread,
                                window,
                                min,
                                max,
                                u32::MAX,
                                self.runtime_input.time_ms,
                                self.runtime_input.cursor,
                            )
                            .is_ok_and(|message| message.is_some())
                        || self.timer_message_ready(thread, window, min, max)
                }
                Err(_) => true,
            },
            _ => false,
        }
    }
    pub(in crate::process) fn check_thread_scratch(
        &self,
        memory: &crate::memory::AddressSpace,
    ) -> Result<(), User32Error32> {
        check_scratch(&self.pending, &self.destructions, memory)
    }
    pub(in crate::process) fn check_owned_thread_scratch(
        &self,
        memory: &crate::memory::AddressSpace,
    ) -> Result<(), User32Error32> {
        check_owned_scratch(&self.pending, &self.destructions, memory)
    }
    pub(in crate::process) fn check_callback_scratch(
        &self,
        continuation: u64,
        memory: &crate::memory::AddressSpace,
    ) -> Result<(), User32Error32> {
        if self
            .pending
            .last()
            .is_none_or(|pending| pending.token != continuation)
        {
            return Err(User32Error32::InvalidToken);
        }
        self.check_thread_scratch(memory)?;
        if let Some(pending) = self.pending.last()
            && let super::super::callback::Continuation32::FileDialog(call) = pending.continuation
        {
            self.check_file_dialog_output(call, memory)?;
        }
        Ok(())
    }
    pub(in crate::process) fn thread_scratch_ranges(
        &self,
    ) -> impl Iterator<Item = crate::memory::PageRange> + '_ {
        self.pending
            .iter()
            .filter_map(|pending| match pending.continuation {
                super::super::callback::Continuation32::FileDialog(call) => {
                    call.scratch.map(|scratch| scratch.range())
                }
                super::super::callback::Continuation32::DialogHelp { scratch, .. }
                | super::super::callback::Continuation32::ResourceDialog { scratch, .. }
                | super::super::callback::Continuation32::MenuPopup { scratch, .. }
                | super::super::callback::Continuation32::DialogRemoteHelp { scratch, .. }
                | super::super::callback::Continuation32::Create { scratch, .. }
                | super::super::callback::Continuation32::Position { scratch, .. }
                | super::super::callback::Continuation32::Style { scratch, .. } => {
                    Some(scratch.range())
                }
                super::super::callback::Continuation32::StatusDraw(call) => {
                    Some(call.scratch().range())
                }
                super::super::callback::Continuation32::Numeric(call) => {
                    Some(call.scratch().range())
                }
                super::super::callback::Continuation32::Header(call) => {
                    Some(call.scratch().range())
                }
                super::super::callback::Continuation32::ListView(call) => {
                    call.scratch().map(|scratch| scratch.range())
                }
                super::super::callback::Continuation32::TabSelection(call) => {
                    Some(call.scratch().range())
                }
                super::super::callback::Continuation32::TooltipText(call) => {
                    Some(call.scratch().range())
                }
                super::super::callback::Continuation32::TooltipTrack(call) => {
                    call.scratch().map(|scratch| scratch.range())
                }
                super::super::callback::Continuation32::MenuBar(call) => {
                    Some(call.scratch().range())
                }
                _ => None,
            })
            .chain(
                self.destructions
                    .iter()
                    .filter_map(DestroyPlan32::scratch_range),
            )
    }
    pub(in crate::process) fn empty_thread_reply(&self) -> User32ThreadReply32 {
        User32ThreadReply32 {
            image: None,
            pending: Vec::new(),
            destructions: Vec::new(),
            last_error: None,
            input: ThreadInput32 {
                queued: [0; 256],
                message: Message32::default(),
                dead: None,
                extra_info: 0,
                layout: self.input.layout,
                layout_revision: self.process_layout.map_or(0, |policy| policy.0),
                show_count: 0,
            },
        }
    }
    pub(in crate::process) fn take_thread_reply(&mut self) -> User32ThreadReply32 {
        User32ThreadReply32 {
            image: self.images.take_reply(),
            pending: std::mem::take(&mut self.pending),
            destructions: std::mem::take(&mut self.destructions),
            last_error: self.last_error.take(),
            input: ThreadInput32 {
                queued: self.input.queued,
                message: self.input.message,
                dead: self.input.dead,
                extra_info: self.input.extra_info,
                layout: self.input.layout,
                layout_revision: self.process_layout.map_or(0, |policy| policy.0),
                show_count: self.input.show_count,
            },
        }
    }
    pub(in crate::process) fn restore_thread_reply(&mut self, reply: User32ThreadReply32) {
        assert!(
            self.pending.is_empty() && self.destructions.is_empty(),
            "active USER32 continuation was parked before restoring another thread"
        );
        let layout = self.resolve_thread_layout(&reply);
        self.images.restore_reply(reply.image);
        self.pending = reply.pending;
        self.destructions = reply.destructions;
        self.last_error = reply.last_error;
        self.input.queued = reply.input.queued;
        self.input.message = reply.input.message;
        self.input.dead = reply.input.dead;
        self.input.extra_info = reply.input.extra_info;
        self.input.layout = layout;
        if self.input.layout != reply.input.layout {
            self.input.dead = None;
        }
        self.input.show_count = reply.input.show_count;
    }
    pub(in crate::process::windows::user32) fn prepare_thread_slots(
        &mut self,
    ) -> Result<(), User32Error32> {
        if self.pending.capacity() < self.continuation_limit {
            self.pending
                .try_reserve_exact(self.continuation_limit - self.pending.len())
                .map_err(|_| User32Error32::Allocation)?;
        }
        if self.destructions.capacity() < self.continuation_limit {
            self.destructions
                .try_reserve_exact(self.continuation_limit - self.destructions.len())
                .map_err(|_| User32Error32::Allocation)?;
        }
        Ok(())
    }
}

impl User32ThreadReply32 {
    pub(in crate::process::windows::user32) fn image_reply(
        &self,
    ) -> Option<&super::super::images::ImageReply32> {
        self.image.as_ref()
    }
    pub(in crate::process) fn owns_clipboard_render(
        &self,
        child: u64,
        window: u32,
        format: u32,
    ) -> bool {
        super::super::clipboard::owns_render(&self.pending, child, window, format)
    }

    pub(in crate::process) fn check_owned_thread_scratch(
        &self,
        memory: &crate::memory::AddressSpace,
    ) -> Result<(), User32Error32> {
        check_owned_scratch(&self.pending, &self.destructions, memory)
    }
    pub(super) fn layout_revision(&self) -> u64 {
        self.input.layout_revision
    }
    pub(super) fn layout(&self) -> KeyboardLayout32 {
        self.input.layout
    }
    pub(in crate::process) fn scratch_ranges(
        &self,
    ) -> impl Iterator<Item = crate::memory::PageRange> + '_ {
        self.pending
            .iter()
            .filter_map(|pending| match pending.continuation {
                super::super::callback::Continuation32::FileDialog(call) => {
                    call.scratch.map(|scratch| scratch.range())
                }
                super::super::callback::Continuation32::DialogHelp { scratch, .. }
                | super::super::callback::Continuation32::ResourceDialog { scratch, .. }
                | super::super::callback::Continuation32::MenuPopup { scratch, .. }
                | super::super::callback::Continuation32::DialogRemoteHelp { scratch, .. }
                | super::super::callback::Continuation32::Create { scratch, .. }
                | super::super::callback::Continuation32::Position { scratch, .. }
                | super::super::callback::Continuation32::Style { scratch, .. } => {
                    Some(scratch.range())
                }
                super::super::callback::Continuation32::StatusDraw(call) => {
                    Some(call.scratch().range())
                }
                super::super::callback::Continuation32::Numeric(call) => {
                    Some(call.scratch().range())
                }
                super::super::callback::Continuation32::Header(call) => {
                    Some(call.scratch().range())
                }
                super::super::callback::Continuation32::ListView(call) => {
                    call.scratch().map(|scratch| scratch.range())
                }
                super::super::callback::Continuation32::TabSelection(call) => {
                    Some(call.scratch().range())
                }
                super::super::callback::Continuation32::TooltipText(call) => {
                    Some(call.scratch().range())
                }
                super::super::callback::Continuation32::TooltipTrack(call) => {
                    call.scratch().map(|scratch| scratch.range())
                }
                super::super::callback::Continuation32::MenuBar(call) => {
                    Some(call.scratch().range())
                }
                _ => None,
            })
            .chain(
                self.destructions
                    .iter()
                    .filter_map(DestroyPlan32::scratch_range),
            )
    }

    pub(in crate::process) fn check_remote_result(
        &self,
        continuation: u64,
    ) -> Result<(), User32Error32> {
        let pending = self
            .pending
            .iter()
            .find(|pending| pending.token == continuation)
            .ok_or(User32Error32::InvalidToken)?;
        match pending.continuation {
            super::super::callback::Continuation32::RemoteCall { ready: None, .. }
            | super::super::callback::Continuation32::DialogRemoteHelp { ready: None, .. } => {
                Ok(())
            }
            _ => Err(User32Error32::InvalidToken),
        }
    }
    pub(in crate::process) fn check_remote_scratch(
        &self,
        continuation: u64,
        memory: &crate::memory::AddressSpace,
    ) -> Result<(), User32Error32> {
        self.check_remote_result(continuation)?;
        let pending = self
            .pending
            .iter()
            .find(|pending| pending.token == continuation)
            .unwrap();
        if let super::super::callback::Continuation32::DialogRemoteHelp { scratch, .. } =
            pending.continuation
        {
            scratch.check(memory)?;
        }
        Ok(())
    }
    pub(in crate::process) fn accept_remote_result(
        &mut self,
        continuation: u64,
        value: u32,
    ) -> Result<(), User32Error32> {
        let pending = self
            .pending
            .iter_mut()
            .find(|pending| pending.token == continuation)
            .ok_or(User32Error32::InvalidToken)?;
        let ready = match &mut pending.continuation {
            super::super::callback::Continuation32::RemoteCall { ready, .. }
            | super::super::callback::Continuation32::DialogRemoteHelp { ready, .. } => ready,
            _ => return Err(User32Error32::InvalidToken),
        };
        if ready.is_some() {
            return Err(User32Error32::InvalidToken);
        }
        *ready = Some(value);
        Ok(())
    }
}

fn check_scratch(
    pending: &[Pending32],
    destructions: &[DestroyPlan32],
    memory: &crate::memory::AddressSpace,
) -> Result<(), User32Error32> {
    check_owned_scratch(pending, destructions, memory)?;
    for pending in pending {
        match pending.continuation {
            super::super::callback::Continuation32::Paint { output, .. } => output.check(memory)?,
            super::super::callback::Continuation32::QueryPaint(paint) => {
                paint.check_output(memory)?
            }
            super::super::callback::Continuation32::SentPaint(paint) => {
                paint.check_output(memory)?
            }
            super::super::callback::Continuation32::StatusDraw(call) => {
                call.scratch().check(memory)?
            }
            super::super::callback::Continuation32::Numeric(call) => {
                call.scratch().check(memory)?;
                call.check_output(memory)?;
            }
            super::super::callback::Continuation32::Header(call) => call.scratch().check(memory)?,
            super::super::callback::Continuation32::ListView(call) => {
                if let Some(scratch) = call.scratch() {
                    scratch.check(memory)?;
                }
            }
            super::super::callback::Continuation32::TabSelection(call) => {
                call.scratch().check(memory)?
            }
            super::super::callback::Continuation32::TooltipText(call) => {
                call.scratch().check(memory)?;
            }
            super::super::callback::Continuation32::TooltipTrack(call) => {
                if let Some(scratch) = call.scratch() {
                    scratch.check(memory)?;
                }
            }
            super::super::callback::Continuation32::MenuBar(call) => call.check(memory)?,
            _ => {}
        }
    }
    Ok(())
}

fn check_owned_scratch(
    pending: &[Pending32],
    destructions: &[DestroyPlan32],
    memory: &crate::memory::AddressSpace,
) -> Result<(), User32Error32> {
    for pending in pending {
        match pending.continuation {
            super::super::callback::Continuation32::FileDialog(call) => {
                if let Some(scratch) = call.scratch {
                    scratch.check(memory)?;
                }
            }
            super::super::callback::Continuation32::DialogHelp { scratch, .. }
            | super::super::callback::Continuation32::ResourceDialog { scratch, .. }
            | super::super::callback::Continuation32::MenuPopup { scratch, .. }
            | super::super::callback::Continuation32::DialogRemoteHelp { scratch, .. }
            | super::super::callback::Continuation32::Create { scratch, .. }
            | super::super::callback::Continuation32::Position { scratch, .. }
            | super::super::callback::Continuation32::Style { scratch, .. } => {
                scratch.check(memory)?
            }
            super::super::callback::Continuation32::StatusDraw(call) => {
                call.scratch().check(memory)?
            }
            super::super::callback::Continuation32::Numeric(call) => {
                call.scratch().check(memory)?
            }
            super::super::callback::Continuation32::Header(call) => call.scratch().check(memory)?,
            super::super::callback::Continuation32::ListView(call) => {
                if let Some(scratch) = call.scratch() {
                    scratch.check(memory)?;
                }
            }
            super::super::callback::Continuation32::TabSelection(call) => {
                call.scratch().check(memory)?
            }
            super::super::callback::Continuation32::TooltipText(call) => {
                call.scratch().check(memory)?;
            }
            super::super::callback::Continuation32::TooltipTrack(call) => {
                if let Some(scratch) = call.scratch() {
                    scratch.check(memory)?;
                }
            }
            super::super::callback::Continuation32::MenuBar(call) => {
                call.scratch().check(memory)?
            }
            _ => {}
        }
    }
    for plan in destructions {
        plan.check_scratch(memory)?;
    }
    Ok(())
}
