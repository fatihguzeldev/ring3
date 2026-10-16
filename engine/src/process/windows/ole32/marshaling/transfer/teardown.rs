use super::context::{TransferParent32, TransferPhase32 as P};
use crate::{
    process::{EngineInstance, HostError, windows::ole32::marshaling::Remote32},
    windows::ole32::ApartmentId32,
};
pub(in crate::process::windows::ole32) struct PreparedTransferExit32 {
    thread: u32,
    changes: Vec<TransferExit32>,
}
struct TransferExit32 {
    continuation: u64,
    sender_dies: bool,
    active: bool,
    target_dies: bool,
}
impl EngineInstance {
    pub(in crate::process::windows::ole32) fn prepare_com_transfer_exit(
        &self,
        thread: u32,
    ) -> Result<PreparedTransferExit32, HostError> {
        let mut changes = Vec::new();
        if let Some(runtime) = &self.ole32 {
            changes
                .try_reserve_exact(runtime.marshaling.transfers.len())
                .map_err(|_| HostError::Infrastructure)?;
            for call in &runtime.marshaling.transfers {
                let sender_dies = call.sender == thread;
                let target_dies =
                    call.remote == Remote32::Queued && call.execution.thread == thread;
                let sta_dies = call.references.iter().any(|reference| {
                    reference.pointer != 0
                        && reference.execution.apartment == ApartmentId32::Sta(thread)
                });
                if !sender_dies && !target_dies && !sta_dies {
                    continue;
                }
                if let Some(scratch) = call.scratch {
                    scratch.check(self.memory()?).map_err(HostError::Memory)?;
                }
                self.com_transfer_parent_scratch(call.parent)?
                    .check(self.memory()?)
                    .map_err(HostError::Memory)?;
                let active = self.has_active_com_apartment_call(call.sender, call.continuation)
                    && !target_dies;
                changes.push(TransferExit32 {
                    continuation: call.continuation,
                    sender_dies,
                    active,
                    target_dies,
                });
            }
        }
        Ok(PreparedTransferExit32 { thread, changes })
    }
    pub(in crate::process::windows::ole32) fn commit_com_transfer_exit(
        &mut self,
        prepared: PreparedTransferExit32,
    ) {
        let Some(runtime) = &mut self.ole32 else {
            return;
        };
        for change in prepared.changes {
            let call = runtime
                .marshaling
                .transfers
                .iter_mut()
                .find(|call| call.continuation == change.continuation)
                .unwrap();
            for reference in &mut call.references {
                if reference.execution.apartment == ApartmentId32::Sta(prepared.thread) {
                    reference.pointer = 0;
                }
            }
            if (call.source.apartment == ApartmentId32::Sta(prepared.thread)
                || call.client.apartment == ApartmentId32::Sta(prepared.thread))
                && !call.dead_sta.contains(&prepared.thread)
                && let Some(empty) = call.dead_sta.iter_mut().find(|thread| **thread == 0)
            {
                *empty = prepared.thread;
            }
            if call.source.apartment == ApartmentId32::Sta(prepared.thread) {
                call.packet = false;
            }
            call.abandoned |= change.sender_dies;
            call.cancelled = true;
            call.output = 0;
            call.result = 0x8001_0108;
            if call.phase == P::Complete {
                call.phase = P::Release(0);
            }
            if change.target_dies
                || (change.sender_dies && !change.active && call.remote == Remote32::Queued)
            {
                call.remote = Remote32::Idle;
            }
        }
    }
    fn com_transfer_parent_scratch(
        &self,
        parent: TransferParent32,
    ) -> Result<crate::process::windows::callbacks::GuestCallbackScratch32, HostError> {
        let runtime = self.ole32.as_ref().ok_or(HostError::Infrastructure)?;
        match parent {
            TransferParent32::Activation(id) => runtime
                .activation
                .pending
                .iter()
                .find(|pending| pending.continuation == id)
                .map(|pending| pending.scratch),
            TransferParent32::RpcQuery(id) => {
                return self.rpc_parent_scratch(id).ok_or(HostError::Infrastructure);
            }
            TransferParent32::Proxy(id) => runtime
                .marshaling
                .calls
                .iter()
                .find(|pending| pending.continuation == id)
                .map(|pending| pending.scratch),
        }
        .ok_or(HostError::Infrastructure)
    }
    pub(in crate::process::windows::ole32) fn abandoned_com_transfer_ready(&self) -> bool {
        self.ole32.as_ref().is_some_and(|runtime| {
            runtime
                .marshaling
                .transfers
                .iter()
                .any(|call| call.abandoned && call.remote != Remote32::Queued)
        })
    }
    pub(in crate::process::windows::ole32) fn service_abandoned_com_transfer(
        &mut self,
    ) -> Result<bool, HostError> {
        let Some(index) = self.ole32.as_ref().and_then(|runtime| {
            runtime
                .marshaling
                .transfers
                .iter()
                .position(|call| call.abandoned && call.remote != Remote32::Queued)
        }) else {
            return Ok(false);
        };
        self.advance_com_transfer(index)?;
        let call = &self.ole32.as_ref().unwrap().marshaling.transfers[index];
        if call.phase == P::Complete {
            let parent = call.parent;
            let scratch = self.com_transfer_parent_scratch(parent)?;
            scratch
                .release(self.memory.as_mut().unwrap())
                .map_err(HostError::Memory)?;
            let runtime = self.ole32.as_mut().unwrap();
            match parent {
                TransferParent32::Activation(id) => {
                    runtime
                        .activation
                        .pending
                        .retain(|pending| pending.continuation != id);
                    runtime
                        .marshaling
                        .activations
                        .retain(|remote| remote.continuation != id);
                }
                TransferParent32::RpcQuery(id) => runtime.rpc_retire_abandoned_parent(id),
                TransferParent32::Proxy(id) => runtime
                    .marshaling
                    .calls
                    .retain(|pending| pending.continuation != id),
            }
            runtime.marshaling.transfers.remove(index);
        }
        Ok(true)
    }
}
