use super::context::RpcRundown32;
use crate::{
    process::{
        EngineInstance, HostError,
        windows::ole32::marshaling::{ExecutionApartment32, Remote32},
    },
    windows::ole32::ApartmentId32,
};
pub(in crate::process::windows::ole32) struct PreparedRpcRundown32 {
    rows: Vec<(usize, RpcRundown32)>,
    next: u64,
}
impl PreparedRpcRundown32 {
    pub(in crate::process::windows::ole32) fn empty(next: u64) -> Self {
        Self {
            rows: Vec::new(),
            next,
        }
    }
    pub(in crate::process::windows::ole32) fn next(&self) -> u64 {
        self.next
    }
}
impl EngineInstance {
    pub(in crate::process::windows::ole32) fn prepare_rpc_rundown(
        &self,
        thread: u32,
        retiring: bool,
        after: u64,
    ) -> Result<PreparedRpcRundown32, HostError> {
        let mut result = PreparedRpcRundown32::empty(after);
        let Some(runtime) = &self.ole32 else {
            return Ok(result);
        };
        let apartment = runtime.apartments.identity(thread).ok();
        result
            .rows
            .try_reserve_exact(runtime.rpc.brokers.len())
            .map_err(|_| HostError::Infrastructure)?;
        for (index, broker) in runtime
            .rpc
            .brokers
            .iter()
            .enumerate()
            .filter_map(|(index, broker)| broker.as_ref().map(|broker| (index, broker)))
        {
            if Some(broker.client.apartment) == apartment
                && broker.build == super::context::BuildPhase32::Publish
                && broker.rundown.is_none()
                && (broker.proxy != 0 || broker.stub != 0 || broker.owns_source)
                && !runtime.rpc.calls.iter().any(|call| {
                    call.broker == index
                        && matches!(call.phase, super::context::RpcPhase32::Cleanup(_))
                })
            {
                result.next = result
                    .next
                    .checked_add(1)
                    .ok_or(HostError::Infrastructure)?;
                result.rows.push((
                    index,
                    RpcRundown32 {
                        continuation: result.next,
                        sender: thread,
                        next: 0,
                        remote: Remote32::Idle,
                        execution: broker.client,
                        retiring,
                    },
                ));
            }
        }
        Ok(result)
    }
    pub(in crate::process::windows::ole32) fn commit_rpc_rundown(
        &mut self,
        prepared: PreparedRpcRundown32,
    ) {
        let Some(runtime) = &mut self.ole32 else {
            return;
        };
        for (index, rundown) in prepared.rows {
            let broker = runtime.rpc.brokers[index].as_mut().unwrap();
            broker.connected = false;
            if rundown.retiring && broker.client.apartment == ApartmentId32::Sta(rundown.sender) {
                broker.proxy = 0;
                broker.interface = 0;
            }
            broker.rundown = Some(rundown);
        }
    }
    pub(in crate::process::windows::ole32) fn rpc_rundown_pending(&self, thread: u32) -> bool {
        self.ole32.as_ref().is_some_and(|runtime| {
            runtime.rpc.brokers.iter().flatten().any(|broker| {
                broker
                    .rundown
                    .is_some_and(|rundown| rundown.sender == thread)
            })
        })
    }
    pub(in crate::process::windows::ole32) fn rpc_rundown_work_ready(&self) -> bool {
        self.ole32.as_ref().is_some_and(|runtime| {
            runtime
                .rpc
                .brokers
                .iter()
                .enumerate()
                .any(|(index, broker)| {
                    broker.as_ref().is_some_and(|broker| {
                        broker
                            .rundown
                            .is_some_and(|rundown| rundown.remote != Remote32::Queued)
                            && !runtime.rpc.calls.iter().any(|call| call.broker == index)
                    })
                })
        })
    }
    pub(in crate::process::windows::ole32) fn service_rpc_rundown(
        &mut self,
    ) -> Result<bool, HostError> {
        let Some(index) = self.ole32.as_ref().and_then(|runtime| {
            runtime
                .rpc
                .brokers
                .iter()
                .enumerate()
                .position(|(index, broker)| {
                    broker.as_ref().is_some_and(|broker| {
                        broker
                            .rundown
                            .is_some_and(|rundown| rundown.remote != Remote32::Queued)
                            && !runtime.rpc.calls.iter().any(|call| call.broker == index)
                    })
                })
        }) else {
            return Ok(false);
        };
        loop {
            let broker = *self.ole32.as_ref().unwrap().rpc.brokers[index]
                .as_ref()
                .unwrap();
            let mut rundown = broker.rundown.unwrap();
            let (pointer, slot, mut execution) = match rundown.next {
                0 => (broker.proxy, 4, broker.client),
                1 => (broker.proxy, 2, broker.client),
                2 => (broker.stub, 4, broker.source),
                3 => (broker.stub, 2, broker.source),
                4 => (
                    if broker.owns_source {
                        broker.source_object
                    } else {
                        0
                    },
                    2,
                    broker.source,
                ),
                _ => {
                    self.finish_rpc_rundown(index)?;
                    return Ok(true);
                }
            };
            if pointer == 0 {
                self.ole32.as_mut().unwrap().rpc.brokers[index]
                    .as_mut()
                    .unwrap()
                    .rundown
                    .as_mut()
                    .unwrap()
                    .next += 1;
                continue;
            }
            if matches!(
                execution.apartment,
                ApartmentId32::Mta | ApartmentId32::Neutral
            ) && (rundown.retiring || execution.thread != rundown.sender)
            {
                execution.thread = self.ensure_com_apartment_host(false)?;
            }
            let procedure = self.com_guest_method(pointer, slot)?;
            self.queue_com_apartment_invocation(
                execution.thread,
                procedure,
                &[pointer],
                rundown.sender,
                rundown.continuation,
            )?;
            rundown.execution = execution;
            rundown.remote = Remote32::Queued;
            self.ole32.as_mut().unwrap().rpc.brokers[index]
                .as_mut()
                .unwrap()
                .rundown = Some(rundown);
            return Ok(true);
        }
    }
    pub(super) fn rpc_rundown_execution(
        &self,
        sender: u32,
        continuation: u64,
    ) -> Option<ExecutionApartment32> {
        self.ole32
            .as_ref()?
            .rpc
            .brokers
            .iter()
            .flatten()
            .find_map(|broker| {
                broker
                    .rundown
                    .filter(|rundown| {
                        rundown.sender == sender
                            && rundown.continuation == continuation
                            && rundown.remote == Remote32::Queued
                    })
                    .map(|rundown| rundown.execution)
            })
    }
    pub(super) fn complete_rpc_rundown(&mut self, sender: u32, continuation: u64) -> bool {
        let Some(broker) = self.ole32.as_mut().and_then(|runtime| {
            runtime.rpc.brokers.iter_mut().flatten().find(|broker| {
                broker.rundown.is_some_and(|rundown| {
                    rundown.sender == sender
                        && rundown.continuation == continuation
                        && rundown.remote == Remote32::Queued
                })
            })
        }) else {
            return false;
        };
        let rundown = broker.rundown.as_mut().unwrap();
        match rundown.next {
            1 => broker.proxy = 0,
            3 => broker.stub = 0,
            4 => {
                broker.source_object = 0;
                broker.owns_source = false
            }
            _ => {}
        };
        rundown.next += 1;
        rundown.remote = Remote32::Ready(0);
        self.wake_com_apartment_sender(sender);
        true
    }
    fn finish_rpc_rundown(&mut self, index: usize) -> Result<(), HostError> {
        self.retire_rpc_broker_buffers(index)?;
        let broker = self.ole32.as_mut().unwrap().rpc.brokers[index]
            .as_mut()
            .unwrap();
        let sender = broker.rundown.take().unwrap().sender;
        if !broker.controller_live && !broker.channel_live {
            self.ole32.as_mut().unwrap().rpc.brokers[index] = None;
        }
        self.wake_com_apartment_sender(sender);
        Ok(())
    }
}
