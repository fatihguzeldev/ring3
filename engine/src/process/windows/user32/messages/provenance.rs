use super::super::{User32Context32, User32Outcome32, callback::Continuation32};
use crate::windows::{
    activation::CapturedActivation32,
    user32::{Message32, User32Error32},
};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::process::windows::user32) struct MessageIdentity32 {
    pub(in crate::process::windows::user32) timer: bool,
    pub(in crate::process::windows::user32) sequence: u64,
}
struct DeliveredMessage32 {
    identity: MessageIdentity32,
    message: Message32,
    activation: CapturedActivation32,
    dispatched: bool,
}
pub(in crate::process::windows::user32) struct MessageProvenance32 {
    delivered: Vec<DeliveredMessage32>,
    limit: usize,
}
pub(in crate::process::windows::user32) struct PreparedDelivery32 {
    slot: usize,
    identity: MessageIdentity32,
    message: Message32,
    activation: CapturedActivation32,
}
impl MessageProvenance32 {
    pub(in crate::process::windows::user32) fn new(limit: usize) -> Result<Self, User32Error32> {
        let mut delivered = Vec::new();
        delivered
            .try_reserve_exact(limit)
            .map_err(|_| User32Error32::Allocation)?;
        Ok(Self { delivered, limit })
    }
    pub(in crate::process::windows::user32) fn prepare(
        &self,
        identity: MessageIdentity32,
        message: Message32,
        activation: CapturedActivation32,
    ) -> Result<Option<PreparedDelivery32>, User32Error32> {
        if !activation.is_captured() {
            return Ok(None);
        }
        let slot = self
            .delivered
            .iter()
            .position(|record| record.identity == identity)
            .or_else(|| (self.delivered.len() < self.limit).then_some(self.delivered.len()))
            .or_else(|| self.delivered.iter().position(|record| record.dispatched))
            .ok_or(User32Error32::Win32(1816))?;
        Ok(Some(PreparedDelivery32 {
            slot,
            identity,
            message,
            activation,
        }))
    }
    pub(in crate::process::windows::user32) fn commit(
        &mut self,
        prepared: Option<PreparedDelivery32>,
    ) {
        let Some(prepared) = prepared else { return };
        let record = DeliveredMessage32 {
            identity: prepared.identity,
            message: prepared.message,
            activation: prepared.activation,
            dispatched: false,
        };
        if prepared.slot == self.delivered.len() {
            self.delivered.push(record);
        } else {
            self.delivered[prepared.slot] = record;
        }
    }
    pub(in crate::process::windows::user32) fn retire_window(&mut self, window: u32) {
        self.delivered
            .retain(|record| record.message.window != window);
    }
    pub(in crate::process::windows::user32) fn retire_thread(&mut self, thread: u32) {
        self.delivered
            .retain(|record| record.message.thread_id != thread);
    }
    pub(in crate::process::windows::user32) fn clear(&mut self) {
        self.delivered.clear();
    }
}
impl User32Context32 {
    pub(in crate::process::windows::user32) fn start_message_callback(
        &mut self,
        continuation: Continuation32,
        mut message: Message32,
        thread: u32,
    ) -> Result<User32Outcome32, User32Error32> {
        message.thread_id = thread;
        let record = self
            .provenance
            .delivered
            .iter()
            .position(|record| record.message == message && !record.dispatched)
            .or_else(|| {
                self.provenance
                    .delivered
                    .iter()
                    .position(|record| record.message == message)
            });
        let activation = record.map(|index| self.provenance.delivered[index].activation.clone());
        let outcome = self.start(continuation)?;
        if let Some(index) = record {
            self.provenance.delivered[index].dispatched = true;
            self.pending.last_mut().unwrap().activation = activation;
        }
        Ok(outcome)
    }
    pub(in crate::process) fn callback_activation(&self, continuation: u64) -> Option<u32> {
        self.pending
            .last()
            .filter(|pending| pending.token == continuation)
            .and_then(|pending| pending.activation.as_ref())
            .map(CapturedActivation32::handle)
    }
}
