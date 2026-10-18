use super::super::{
    User32CallContext32, User32Context32, User32Outcome32, callback::Continuation32, memory,
    messages::MessageIdentity32,
};
use super::{HookCall32, HookKind32};
use crate::{
    memory::{Access, AddressSpace, GuestAddress},
    windows::user32::{Message32, User32Api32, User32Error32},
};

#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct KeyboardQuery32 {
    pub api: User32Api32,
    pub arguments: [u32; 5],
    pub context: User32CallContext32,
    pub remove: bool,
    pub observed: u32,
}

#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct KeyboardMessage32 {
    pub hook: HookCall32,
    query: KeyboardQuery32,
    message: Message32,
    sequence: u64,
    decision: Option<u32>,
}

impl User32Context32 {
    pub(in crate::process) fn keyboard_queue_may_change(&self, token: u64) -> bool {
        self.pending.last().is_some_and(|pending| {
            pending.token == token && matches!(pending.continuation, Continuation32::Keyboard(_))
        })
    }

    pub(in crate::process::windows::user32) fn begin_keyboard_message(
        &mut self,
        query: KeyboardQuery32,
        index: usize,
        message: Message32,
        memory: &AddressSpace,
    ) -> Result<Option<User32Outcome32>, User32Error32> {
        if !matches!(message.message, 0x100 | 0x101 | 0x104 | 0x105)
            || !self
                .state
                .message_keyboard(index)
                .is_some_and(|record| record.key.is_some())
        {
            return Ok(None);
        }
        let Some(hook) = self
            .hooks
            .first(query.context.thread_id, HookKind32::Keyboard)
        else {
            return Ok(None);
        };
        memory.check_span(GuestAddress(query.arguments[0]), 28, Access::Write)?;
        let (sequence, activation) = self.state.message_activation(index);
        self.provenance.prepare(
            MessageIdentity32 {
                timer: false,
                sequence,
            },
            message,
            activation.clone(),
        )?;
        let call = KeyboardMessage32 {
            hook: HookCall32 {
                kind: HookKind32::Keyboard,
                handle: hook.handle,
                thread: hook.thread,
                procedure: hook.procedure,
                arguments: [
                    if query.remove { 0 } else { 3 },
                    message.wparam,
                    message.lparam,
                ],
            },
            query,
            message,
            sequence,
            decision: None,
        };
        // publish the callback owner before consuming the selected hardware record.
        let outcome = self.start(Continuation32::Keyboard(call))?;
        self.pending.last_mut().unwrap().activation =
            activation.is_captured().then_some(activation);
        self.observe_queue_with_timers(query.context.thread_id, query.observed);
        if query.remove {
            self.input.queued = self.state.remove_message(index).unwrap().state;
            self.raw_input.mark_delivered(query.context.thread_id, 0);
        }
        Ok(Some(outcome))
    }

    pub(in crate::process::windows::user32) fn complete_keyboard_message(
        &mut self,
        mut call: KeyboardMessage32,
        result: u32,
        memory: &mut AddressSpace,
    ) -> Result<User32Outcome32, User32Error32> {
        let result = *call.decision.get_or_insert(result);
        self.pending.last_mut().unwrap().continuation = Continuation32::Keyboard(call);
        if result != 0 {
            if !call.query.remove
                && let Some(index) = self.state.message_index_by_sequence(call.sequence)
            {
                self.input.queued = self.state.remove_message(index).unwrap().state;
                self.raw_input
                    .mark_delivered(call.query.context.thread_id, 0);
            }
            let pending = self.pending.pop().unwrap();
            let outcome = self.retrieve_message(
                call.query.api,
                &call.query.arguments,
                memory,
                call.query.context,
            );
            if outcome.is_err() {
                self.pending.push(pending);
            }
            return outcome;
        }
        let activation = self
            .pending
            .last()
            .unwrap()
            .activation
            .clone()
            .unwrap_or_default();
        let delivery = self.provenance.prepare(
            MessageIdentity32 {
                timer: false,
                sequence: call.sequence,
            },
            call.message,
            activation,
        )?;
        memory::write_message(memory, call.query.arguments[0], call.message)?;
        self.provenance.commit(delivery);
        self.input.message = call.message;
        self.pending.pop();
        Ok(User32Outcome32::Return(1))
    }
}
