use super::{MessageQueue32, MessageSlot32, QueuedKey32, QueuedKeyboard32};
use crate::windows::user32::{Message32, User32Error32};
use std::collections::VecDeque;

impl MessageQueue32 {
    pub(crate) fn new(limit: usize) -> Result<Self, User32Error32> {
        let mut posted = VecDeque::new();
        posted
            .try_reserve_exact(limit)
            .map_err(|_| User32Error32::Allocation)?;
        let mut quits = Vec::new();
        quits
            .try_reserve_exact(limit)
            .map_err(|_| User32Error32::Allocation)?;
        let mut sent = Vec::new();
        sent.try_reserve_exact(limit)
            .map_err(|_| User32Error32::Allocation)?;
        Ok(Self {
            posted,
            quits,
            sent,
            limit,
            generation: 0,
        })
    }
    pub(crate) fn can_post(&self, count: usize) -> Result<(), User32Error32> {
        if count > self.limit - self.posted.len() {
            return Err(User32Error32::Win32(1816));
        }
        self.generation
            .checked_add(count as u64)
            .ok_or(User32Error32::Capacity)?;
        Ok(())
    }
    pub(crate) fn project_input(
        &self,
        thread: u32,
        messages: &[Message32],
    ) -> Result<(u32, u32, u64), User32Error32> {
        self.can_post(messages.len())?;
        let (mut current, mut fresh, generation) = self.status(thread);
        for message in messages
            .iter()
            .filter(|message| message.thread_id == thread)
        {
            let flags = message_mask(message.message, true);
            current |= flags;
            fresh |= flags;
        }
        Ok((current, fresh, generation + messages.len() as u64))
    }
    pub(crate) fn post(&mut self, message: Message32) -> Result<(), User32Error32> {
        self.post_record(message, None, false, Default::default())
    }
    pub(crate) fn post_input(
        &mut self,
        message: Message32,
        key: Option<QueuedKey32>,
        state: [u8; 256],
    ) -> Result<(), User32Error32> {
        self.post_record(
            message,
            Some(QueuedKeyboard32 { key, state }),
            true,
            Default::default(),
        )
    }
    pub(crate) fn post_raw_input(&mut self, message: Message32) -> Result<(), User32Error32> {
        self.post_record(message, None, true, Default::default())
    }
    pub(crate) fn post_captured(
        &mut self,
        message: Message32,
        activation: crate::windows::activation::CapturedActivation32,
    ) -> Result<(), User32Error32> {
        self.post_record(message, None, false, activation)
    }
    pub(crate) fn activation(
        &self,
        index: usize,
    ) -> (u64, crate::windows::activation::CapturedActivation32) {
        let slot = if index < self.limit {
            &self.posted[index]
        } else {
            &self.quits[index - self.limit]
        };
        (slot.id, slot.activation.clone())
    }
    fn post_record(
        &mut self,
        message: Message32,
        keyboard: Option<QueuedKeyboard32>,
        input: bool,
        activation: crate::windows::activation::CapturedActivation32,
    ) -> Result<(), User32Error32> {
        self.can_post(1)?;
        self.posted.push_back(MessageSlot32 {
            message,
            id: self.generation + 1,
            activation,
            keyboard,
            input,
            seen: 0,
        });
        self.generation += 1;
        Ok(())
    }
    pub(crate) fn post_quit(&mut self, message: Message32) -> Result<(), User32Error32> {
        let generation = self
            .generation
            .checked_add(1)
            .ok_or(User32Error32::Capacity)?;
        if let Some(slot) = self
            .quits
            .iter_mut()
            .find(|slot| slot.message.thread_id == message.thread_id)
        {
            *slot = MessageSlot32 {
                message,
                id: generation,
                activation: Default::default(),
                keyboard: None,
                input: false,
                seen: 0,
            };
            self.generation = generation;
            return Ok(());
        }
        if self.quits.len() == self.limit {
            return Err(User32Error32::Capacity);
        }
        self.quits.push(MessageSlot32 {
            message,
            id: generation,
            activation: Default::default(),
            keyboard: None,
            input: false,
            seen: 0,
        });
        self.generation = generation;
        Ok(())
    }
    pub(crate) fn find(
        &self,
        thread: u32,
        mask: u32,
        mut matches: impl FnMut(Message32) -> bool,
    ) -> Option<(usize, Message32)> {
        self.posted
            .iter()
            .enumerate()
            .find(|(_, slot)| {
                slot.message.thread_id == thread
                    && message_mask(slot.message.message, slot.input) & mask != 0
                    && matches(slot.message)
            })
            .map(|(i, slot)| (i, slot.message))
            .or_else(|| {
                self.quits
                    .iter()
                    .enumerate()
                    .find(|(_, slot)| slot.message.thread_id == thread)
                    .map(|(i, slot)| (self.limit + i, slot.message))
            })
    }
    pub(crate) fn remove(&mut self, index: usize) -> Option<QueuedKeyboard32> {
        if index < self.limit {
            self.posted.remove(index).and_then(|slot| slot.keyboard)
        } else {
            self.quits.remove(index - self.limit).keyboard
        }
    }
    pub(crate) fn discard_input(&mut self) {
        self.posted.retain(|slot| !slot.input);
    }
    pub(crate) fn project_without_input(&self, thread: u32) -> (u32, u32, u64) {
        let (mut current, mut fresh) = self.sent_status(thread);
        for slot in self
            .posted
            .iter()
            .chain(&self.quits)
            .filter(|slot| !slot.input && slot.message.thread_id == thread)
        {
            let flags = message_mask(slot.message.message, false);
            current |= flags;
            fresh |= flags & !slot.seen;
        }
        (current, fresh, self.generation)
    }
    pub(crate) fn retire_thread(&mut self, thread: u32) {
        self.posted.retain(|slot| slot.message.thread_id != thread);
        self.quits.retain(|slot| slot.message.thread_id != thread);
        self.sent.retain(|slot| slot.thread != thread);
    }
    pub(crate) fn discard_raw_input(&mut self, thread: u32, ids: &[u32]) {
        self.posted.retain(|slot| {
            slot.message.thread_id != thread
                || slot.message.message != 0xff
                || ids.binary_search(&slot.message.lparam).is_err()
        });
    }
    pub(crate) fn retain_window(&mut self, window: u32) {
        self.posted.retain(|slot| slot.message.window != window);
    }
    pub(crate) fn status(&self, thread: u32) -> (u32, u32, u64) {
        let (mut current, mut fresh) = self.sent_status(thread);
        for slot in self
            .posted
            .iter()
            .chain(&self.quits)
            .filter(|slot| slot.message.thread_id == thread)
        {
            let flags = message_mask(slot.message.message, slot.input);
            current |= flags;
            fresh |= flags & !slot.seen;
        }
        (current, fresh, self.generation)
    }
    pub(crate) fn observe(&mut self, thread: u32, mask: u32) {
        if mask & 0x40 != 0
            && let Some(sent) = self.sent.iter_mut().find(|sent| sent.thread == thread)
        {
            sent.fresh = false;
        }
        for slot in self
            .posted
            .iter_mut()
            .chain(&mut self.quits)
            .filter(|slot| slot.message.thread_id == thread)
        {
            slot.seen |= message_mask(slot.message.message, slot.input) & mask;
        }
    }
}

fn message_mask(message: u32, input: bool) -> u32 {
    if input {
        return match message {
            0xff => 0x400,
            0x100..=0x109 => 1,
            0x200 => 2,
            0x201..=0x20e => 4,
            _ => 8 | 0x100,
        };
    }
    match message {
        0xf => 0x20,
        0x113 => 0x10,
        0x312 => 0x80,
        _ => 8 | 0x100,
    }
}
