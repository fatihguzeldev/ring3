use super::{
    context::{TransferParent32, TransferPhase32},
    requests::TransferRequest32,
};
use crate::{
    memory::{GuestAddress, WordWrite32},
    process::{
        EngineInstance, HostError,
        windows::ole32::{activation::Phase32, marshaling::context::ProxyPhase32},
    },
    windows::{
        com::IID_IUNKNOWN,
        ole32::{IID_ICLASSFACTORY, Ole32Api32},
    },
};
impl EngineInstance {
    pub(in crate::process::windows::ole32) fn advance_com_activation_transfer(
        &mut self,
        slot: usize,
        returned: u32,
    ) -> Result<bool, HostError> {
        let runtime = self.ole32.as_ref().unwrap();
        let pending = &runtime.activation.pending[slot];
        let Some(remote) = runtime
            .marshaling
            .activations
            .iter()
            .find(|remote| remote.continuation == pending.continuation)
        else {
            return Ok(false);
        };
        let result = match pending.phase {
            Phase32::Factory if pending.api == Ole32Api32::CoGetClassObject => returned,
            Phase32::Release { result } => result,
            _ => return Ok(false),
        };
        let requested =
            super::super::super::memory::iid(self.memory()?, pending.scratch.address.0 + 32)
                .map_err(|_| HostError::InvalidRequest)?;
        if result & 0x8000_0000 != 0 || matches!(requested, IID_IUNKNOWN | IID_ICLASSFACTORY) {
            return Ok(false);
        }
        let parent = TransferParent32::Activation(pending.continuation);
        if self.com_transfer_result(parent).is_some() {
            return Ok(false);
        }
        if let Some(index) = self.com_transfer_index(parent) {
            self.advance_com_transfer(index)?;
            return Ok(true);
        }
        let pointer = self.ole32_read_word(pending.scratch.address.0 + 52)?;
        if pointer == 0 {
            return Ok(false);
        }
        let request = TransferRequest32 {
            parent,
            source: remote.execution,
            client: remote.client,
            sender: pending.thread,
            owner: pending.owner,
            token: pending.token,
            frame: pending.frame,
            pointer,
            requested,
        };
        self.begin_com_transfer(request)?;
        Ok(true)
    }
    pub(in crate::process::windows::ole32) fn begin_com_proxy_transfer(
        &mut self,
        index: usize,
        result: u32,
        pointer: u32,
        requested: crate::windows::com::InterfaceId,
    ) -> Result<(), HostError> {
        let runtime = self.ole32.as_ref().unwrap();
        let call = &runtime.marshaling.calls[index];
        let proxy = runtime.marshaling.proxies[call.proxy].as_ref().unwrap();
        let request = TransferRequest32 {
            parent: TransferParent32::Proxy(call.continuation),
            source: call.execution,
            client: proxy.client,
            sender: call.thread,
            owner: call.owner,
            token: call.token,
            frame: call.frame,
            pointer,
            requested,
        };
        self.begin_com_transfer(request)?;
        let call = &mut self.ole32.as_mut().unwrap().marshaling.calls[index];
        call.phase = ProxyPhase32::Transfer { result };
        call.remote = super::super::Remote32::Idle;
        Ok(())
    }
    pub(in crate::process::windows::ole32) fn finish_com_proxy_transfer(
        &mut self,
        index: usize,
    ) -> Result<(), HostError> {
        let call = &self.ole32.as_ref().unwrap().marshaling.calls[index];
        let parent = TransferParent32::Proxy(call.continuation);
        self.check_com_transfer_parent(parent)?;
        let Some((result, pointer)) = self.com_transfer_result(parent) else {
            return Ok(());
        };
        call.scratch
            .check(self.memory()?)
            .map_err(HostError::Memory)?;
        let frame = call.frame;
        let prepared = Self::prepare_completed_call(frame.complete(result))?;
        let word = WordWrite32 {
            address: GuestAddress(call.out),
            value: if result & 0x8000_0000 == 0 {
                pointer
            } else {
                0
            },
        };
        self.memory
            .as_mut()
            .unwrap()
            .unmap_ranges_with_word_writes(&[call.scratch.range()], &[word])
            .map_err(HostError::Memory)?;
        self.finish_com_transfer_metadata(parent);
        self.ole32.as_mut().unwrap().marshaling.calls.remove(index);
        self.publish_prepared_call(prepared);
        Ok(())
    }
    pub(in crate::process::windows::ole32) fn com_transfer_parent_ready(
        &self,
        parent: TransferParent32,
    ) -> Option<bool> {
        self.com_transfer_index(parent).map(|index| {
            self.ole32.as_ref().unwrap().marshaling.transfers[index].phase
                == TransferPhase32::Complete
        })
    }
    pub(in crate::process::windows::ole32) fn com_transfer_work_ready(&self, thread: u32) -> bool {
        self.ole32.as_ref().is_some_and(|runtime| {
            runtime.marshaling.transfers.iter().any(|call| {
                !call.abandoned
                    && call.sender == thread
                    && call.phase != TransferPhase32::Complete
                    && call.remote != super::super::Remote32::Queued
            })
        })
    }
    pub(in crate::process::windows::ole32) fn resume_com_transfer_work(
        &mut self,
    ) -> Result<bool, HostError> {
        let Some(pending) = self.pending_call.as_ref() else {
            return Ok(false);
        };
        let Some(index) = self.ole32.as_ref().and_then(|runtime| {
            runtime.marshaling.transfers.iter().position(|call| {
                !call.abandoned
                    && call.owner == pending.owner
                    && call.token == pending.token
                    && call.phase != TransferPhase32::Complete
            })
        }) else {
            return Ok(false);
        };
        let call = &self.ole32.as_ref().unwrap().marshaling.transfers[index];
        if self.checked_pending_call(call.owner, call.token)?.frame != call.frame {
            return Err(HostError::InvalidRequest);
        }
        self.advance_com_transfer(index)?;
        if self.ole32.as_ref().unwrap().marshaling.transfers[index].phase
            == TransferPhase32::Complete
        {
            match self.ole32.as_ref().unwrap().marshaling.transfers[index].parent {
                TransferParent32::Activation(id) => {
                    let slot = self
                        .ole32
                        .as_ref()
                        .unwrap()
                        .activation
                        .pending
                        .iter()
                        .position(|pending| pending.continuation == id)
                        .ok_or(HostError::Infrastructure)?;
                    self.advance_registered_com_loading(slot)?;
                }
                TransferParent32::Proxy(id) => {
                    let slot = self
                        .ole32
                        .as_ref()
                        .unwrap()
                        .marshaling
                        .calls
                        .iter()
                        .position(|pending| pending.continuation == id)
                        .ok_or(HostError::Infrastructure)?;
                    self.finish_com_proxy_transfer(slot)?;
                }
            }
        }
        Ok(true)
    }
}
