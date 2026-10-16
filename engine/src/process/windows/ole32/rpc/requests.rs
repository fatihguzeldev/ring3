use super::context::{RpcCall32, RpcPhase32};
use crate::{
    memory::GuestAddress,
    process::{
        EngineInstance, HostError,
        calls::call::PendingOwner,
        windows::{
            callbacks::GuestCallbackScratch32,
            ole32::marshaling::{
                Remote32,
                transfer::{TransferParent32, TransferRequest32},
            },
        },
    },
    windows::{CallFrame32, com::InterfaceId},
};
impl EngineInstance {
    pub(super) fn begin_rpc_query(
        &mut self,
        owner: PendingOwner,
        token: u32,
        frame: CallFrame32,
        broker: usize,
        requested: InterfaceId,
    ) -> Result<(), HostError> {
        let continuation = self.reserve_rpc_call()?;
        let output = frame.arguments()[2];
        self.memory()?
            .check_span(GuestAddress(output), 4, crate::memory::Access::Write)
            .map_err(HostError::Memory)?;
        let execution = self.ole32.as_ref().unwrap().rpc.brokers[broker]
            .as_ref()
            .unwrap()
            .source;
        let mut bytes = [0; 32];
        bytes[..16].copy_from_slice(&requested.0);
        let scratch = GuestCallbackScratch32::allocate(
            self.memory.as_mut().unwrap(),
            super::super::activation::ActivationRuntime32::scratch_reservation(),
            &bytes,
        )
        .map_err(HostError::Memory)?;
        let call = RpcCall32 {
            continuation,
            sender: self.threads.current_id(),
            owner,
            token,
            frame,
            broker,
            execution,
            remote: Remote32::Idle,
            phase: RpcPhase32::Query(requested),
            message: output,
            status: 0,
            scratch: Some(scratch),
            request_buffer: 0,
            abandoned: false,
            cancelled: false,
            returned: 0,
            request: None,
            request_lease: None,
            held_channel: None,
        };
        self.publish_rpc_call(call)
    }
    pub(super) fn start_rpc_query_transfer(
        &mut self,
        index: usize,
        requested: InterfaceId,
        _result: u32,
    ) -> Result<(), HostError> {
        let runtime = self.ole32.as_ref().unwrap();
        let call = &runtime.rpc.calls[index];
        let broker = runtime.rpc.brokers[call.broker].as_ref().unwrap();
        let pointer = self.ole32_read_word(call.scratch.unwrap().address.0 + 16)?;
        let request = TransferRequest32 {
            parent: TransferParent32::RpcQuery(call.continuation),
            source: call.execution,
            client: broker.client.apartment,
            sender: call.sender,
            owner: call.owner,
            token: call.token,
            frame: call.frame,
            pointer,
            requested,
        };
        if pointer == 0 {
            return self.finish_rpc_query(index, 0x8000ffff, 0);
        }
        self.begin_com_transfer(request)?;
        let call = &mut self.ole32.as_mut().unwrap().rpc.calls[index];
        call.phase = RpcPhase32::Transfer;
        call.remote = Remote32::Idle;
        Ok(())
    }
    pub(in crate::process::windows::ole32) fn finish_rpc_query_transfer(
        &mut self,
        continuation: u64,
    ) -> Result<(), HostError> {
        let Some((result, pointer)) =
            self.com_transfer_result(TransferParent32::RpcQuery(continuation))
        else {
            return Ok(());
        };
        let index = self
            .ole32
            .as_ref()
            .unwrap()
            .rpc
            .calls
            .iter()
            .position(|call| call.continuation == continuation)
            .ok_or(HostError::InvalidRequest)?;
        self.finish_rpc_query(index, result, pointer)?;
        self.finish_com_transfer_metadata(TransferParent32::RpcQuery(continuation));
        Ok(())
    }
    pub(super) fn finish_rpc_query(
        &mut self,
        index: usize,
        result: u32,
        pointer: u32,
    ) -> Result<(), HostError> {
        let call = &self.ole32.as_ref().unwrap().rpc.calls[index];
        let prepared = Self::prepare_completed_call(call.frame.complete(result))?;
        let scratch = call.scratch.unwrap();
        scratch.check(self.memory()?).map_err(HostError::Memory)?;
        self.memory
            .as_mut()
            .unwrap()
            .unmap_ranges_with_word_writes(
                &[scratch.range()],
                &[crate::memory::WordWrite32 {
                    address: GuestAddress(call.message),
                    value: if result & 0x80000000 == 0 { pointer } else { 0 },
                }],
            )
            .map_err(HostError::Memory)?;
        self.ole32.as_mut().unwrap().rpc.calls.remove(index);
        self.publish_prepared_call(prepared);
        Ok(())
    }
    pub(in crate::process::windows::ole32) fn rpc_parent_identity(
        &self,
        id: u64,
    ) -> Option<(PendingOwner, u32, CallFrame32, u32)> {
        self.ole32
            .as_ref()?
            .rpc
            .calls
            .iter()
            .find(|call| call.continuation == id)
            .map(|call| (call.owner, call.token, call.frame, call.sender))
    }
    pub(in crate::process::windows::ole32) fn rpc_parent_scratch(
        &self,
        id: u64,
    ) -> Option<GuestCallbackScratch32> {
        self.ole32
            .as_ref()?
            .rpc
            .calls
            .iter()
            .find(|call| call.continuation == id)?
            .scratch
    }
}

impl crate::process::windows::ole32::Ole32Runtime32 {
    pub(in crate::process::windows::ole32) fn rpc_retire_abandoned_parent(&mut self, id: u64) {
        self.rpc.calls.retain(|call| call.continuation != id);
    }
}
