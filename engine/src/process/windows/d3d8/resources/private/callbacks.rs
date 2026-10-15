use super::{PendingPrivateCall, Phase, PrivateCallbackRequest32};
use crate::process::windows::d3d8::{D3D8Runtime32, DispatchError32, memory::read_words};
use crate::{
    graphics::d3d8::api::HResult,
    memory::{Access, AddressSpace, GuestAddress},
};

impl D3D8Runtime32 {
    pub(super) fn private_reference_procedure(
        &self,
        memory: &AddressSpace,
        pointer: GuestAddress,
        slot: u32,
    ) -> Result<Option<GuestAddress>, DispatchError32> {
        if self.com.lookup(pointer).is_ok() {
            self.com.preflight(memory, pointer)?;
            return Ok(None);
        }
        let [vtable] = read_words(memory, pointer.0)?;
        let entry = vtable.checked_add(slot * 4).ok_or(HResult::INVALID_CALL)?;
        let [procedure] = read_words(memory, entry)?;
        if procedure == 0 {
            return Err(HResult::INVALID_CALL.into());
        }
        memory.check_span(GuestAddress(procedure), 1, Access::Execute)?;
        Ok(Some(GuestAddress(procedure)))
    }

    pub(super) fn start_private_call(
        &mut self,
        memory: &mut AddressSpace,
        mut pending: PendingPrivateCall,
    ) -> Result<u32, DispatchError32> {
        if self.private_callbacks.len() >= self.config.max_objects as usize + 32 {
            return Err(HResult::OUT_OF_MEMORY.into());
        }
        self.private_callbacks
            .try_reserve(1)
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        let continuation = self
            .private_sequence
            .checked_add(1)
            .ok_or(HResult::OUT_OF_MEMORY)?;
        self.private_sequence = continuation;
        pending.continuation = continuation;
        self.advance_private_call(memory, pending)
    }

    fn dispatch_private_reference(
        &mut self,
        memory: &mut AddressSpace,
        pending: PendingPrivateCall,
        pointer: GuestAddress,
        add: bool,
    ) -> Result<PendingPrivateCall, DispatchError32> {
        if let Some(procedure) =
            self.private_reference_procedure(memory, pointer, if add { 1 } else { 2 })?
        {
            let continuation = pending.continuation;
            self.private_callbacks.push(pending);
            return Err(DispatchError32::Callback(PrivateCallbackRequest32 {
                procedure,
                arguments: [pointer.0, 0, 0, 0],
                argument_count: 1,
                continuation,
            }));
        }
        self.owned_private_reference(memory, pointer, add)?;
        Ok(pending)
    }

    fn advance_private_call(
        &mut self,
        memory: &mut AddressSpace,
        mut pending: PendingPrivateCall,
    ) -> Result<u32, DispatchError32> {
        if matches!(pending.phase, Phase::Add) {
            if let Some(pointer) = pending.acquired {
                pending = self.dispatch_private_reference(memory, pending, pointer, true)?;
            }
            return self.publish_after_private_add(memory, pending);
        }
        loop {
            match pending.phase {
                Phase::Release => {
                    if let Some(pointer) = pending.releases.pop() {
                        pending =
                            self.dispatch_private_reference(memory, pending, pointer, false)?;
                    } else {
                        return Ok(pending.result);
                    }
                }
                Phase::Compensate(error) => {
                    if let Some(pointer) = pending.acquired.take() {
                        pending =
                            self.dispatch_private_reference(memory, pending, pointer, false)?;
                    } else {
                        return Err(error);
                    }
                }
                Phase::Add => unreachable!(),
            }
        }
    }

    fn publish_after_private_add(
        &mut self,
        memory: &mut AddressSpace,
        mut pending: PendingPrivateCall,
    ) -> Result<u32, DispatchError32> {
        match self.publish_private_action(memory, pending.action.take().unwrap()) {
            Ok(()) => {
                pending.phase = Phase::Release;
                pending.acquired = None;
            }
            Err(error) => {
                pending.phase = Phase::Compensate(error);
                pending.releases.clear();
            }
        }
        self.advance_private_call(memory, pending)
    }

    pub(in crate::process::windows::d3d8) fn resume_private_callback(
        &mut self,
        continuation: u64,
        _result: u32,
        memory: &mut AddressSpace,
    ) -> Result<u32, DispatchError32> {
        let index = self
            .private_callbacks
            .iter()
            .position(|pending| pending.continuation == continuation)
            .ok_or(HResult::INVALID_CALL)?;
        let pending = self.private_callbacks.remove(index);
        if matches!(pending.phase, Phase::Add) {
            self.publish_after_private_add(memory, pending)
        } else {
            self.advance_private_call(memory, pending)
        }
    }

    pub(in crate::process::windows::d3d8) fn release_private_callbacks(
        &mut self,
        memory: &mut AddressSpace,
        releases: Vec<GuestAddress>,
        result: u32,
    ) -> Result<u32, DispatchError32> {
        self.start_private_call(
            memory,
            PendingPrivateCall {
                continuation: 0,
                action: None,
                phase: Phase::Release,
                acquired: None,
                releases,
                result,
            },
        )
    }
}
