use super::context::{BuildPhase32, RpcPhase32};
use crate::process::{
    EngineInstance,
    windows::ole32::marshaling::{Remote32, transfer::TransferPhase32},
};

impl super::context::RpcRuntime32 {
    pub(in crate::process::windows::ole32) fn record_destroyed_identities(
        &mut self,
        destroyed: &[crate::windows::com::ComIdentity32],
    ) {
        for identity in destroyed {
            let tag = identity.payload & !(super::context::CONTROLLER_PAYLOAD - 1);
            let index = (identity.payload & (super::context::CONTROLLER_PAYLOAD - 1)) as usize;
            if let Some(broker) = self.brokers.get_mut(index).and_then(Option::as_mut) {
                if tag == super::context::CONTROLLER_PAYLOAD {
                    broker.controller_live = false;
                }
                if tag == super::context::CHANNEL_PAYLOAD {
                    broker.channel_live = false;
                }
            }
        }
    }

    pub(super) fn retire_disconnected_broker(&mut self, index: usize) {
        let dead = self.brokers[index].as_ref().is_some_and(|broker| {
            !broker.controller_live
                && !broker.channel_live
                && broker.proxy == 0
                && broker.stub == 0
                && broker.source_factory == 0
                && broker.client_factory == 0
                && !broker.owns_source
                && broker.rundown.is_none()
        }) && !self.calls.iter().any(|call| call.broker == index);
        if dead {
            self.brokers[index] = None;
        }
    }
}

impl EngineInstance {
    // Only references actually acquired/released inside the native proxy's
    // Connect/Disconnect belong to its channel lease. Other client references
    // keep their own lifetime and are never guessed from the total COM count.
    pub(super) fn rpc_channel_lease_scope(&self, broker: usize, thread: u32) -> bool {
        let Some(runtime) = &self.ole32 else {
            return false;
        };
        let Some(entry) = runtime.rpc.brokers.get(broker).and_then(Option::as_ref) else {
            return false;
        };
        let active = |sender, continuation, execution, remote| {
            execution == thread
                && remote == Remote32::Queued
                && self.active_com_apartment_call(thread) == Some((sender, continuation))
        };
        runtime.marshaling.transfers.iter().any(|call| {
            call.phase == TransferPhase32::ProxyStub(broker)
                && matches!(entry.build, BuildPhase32::Connect | BuildPhase32::Failed(1))
                && active(
                    call.sender,
                    call.continuation,
                    call.execution.thread,
                    call.remote,
                )
        }) || runtime.rpc.calls.iter().any(|call| {
            call.broker == broker
                && call.phase == RpcPhase32::Cleanup(0)
                && active(
                    call.sender,
                    call.continuation,
                    call.execution.thread,
                    call.remote,
                )
        }) || entry.rundown.is_some_and(|call| {
            call.next == 0
                && active(
                    call.sender,
                    call.continuation,
                    call.execution.thread,
                    call.remote,
                )
        })
    }

    pub(super) fn active_rpc_invoke(&self, broker: usize, thread: u32) -> Option<u64> {
        let (sender, continuation) = self.active_com_apartment_call(thread)?;
        self.ole32
            .as_ref()?
            .rpc
            .calls
            .iter()
            .find(|call| {
                call.broker == broker
                    && call.sender == sender
                    && call.continuation == continuation
                    && call.execution.thread == thread
                    && call.remote == Remote32::Queued
                    && call.phase == RpcPhase32::Invoke
            })
            .map(|call| call.continuation)
    }
}
