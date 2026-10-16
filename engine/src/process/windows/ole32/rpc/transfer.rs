use super::context::BuildPhase32 as B;
use crate::{
    process::{
        EngineInstance, HostError,
        windows::ole32::marshaling::{Remote32, transfer::TransferPhase32 as P},
    },
    windows::ole32::{ApartmentId32, HResult32, Ole32Api32},
};
impl EngineInstance {
    pub(in crate::process::windows::ole32) fn advance_registered_ps_transfer(
        &mut self,
        transfer: usize,
        index: usize,
    ) -> Result<(), HostError> {
        let broker = *self.ole32.as_ref().unwrap().rpc.brokers[index]
            .as_ref()
            .ok_or(HostError::Infrastructure)?;
        if broker.build == B::Publish {
            return self.publish_registered_ps_transfer(transfer, index);
        }
        if let B::Failed(next) = broker.build {
            return self.advance_registered_ps_cleanup(transfer, index, next);
        }
        if matches!(broker.build, B::Proxy | B::Connect) {
            self.prepare_rpc_identities(index)?;
        }
        let call = &self.ole32.as_ref().unwrap().marshaling.transfers[transfer];
        let broker = *self.ole32.as_ref().unwrap().rpc.brokers[index]
            .as_ref()
            .unwrap();
        let scratch = call.scratch().address.0;
        let (object, slot, args, count, mut execution, gate) = match broker.build {
            B::SourceFactory => (
                0,
                0,
                [scratch + 32, 1, 0, scratch + 16, scratch + 64, 0],
                5,
                call.source,
                Some(Ole32Api32::CoGetClassObject),
            ),
            B::Stub => (
                broker.source_factory,
                4,
                [
                    broker.source_factory,
                    scratch,
                    broker.source_object,
                    scratch + 68,
                    0,
                    0,
                ],
                4,
                call.source,
                None,
            ),
            B::ClientFactory => (
                0,
                0,
                [scratch + 32, 1, 0, scratch + 16, scratch + 72, 0],
                5,
                call.client,
                Some(Ole32Api32::CoGetClassObject),
            ),
            B::Proxy => (
                broker.client_factory,
                3,
                [
                    broker.client_factory,
                    broker.controller.unwrap().0,
                    scratch,
                    scratch + 76,
                    scratch + 80,
                    0,
                ],
                5,
                call.client,
                None,
            ),
            B::Connect => (
                broker.proxy,
                3,
                [broker.proxy, broker.channel.unwrap().0, 0, 0, 0, 0],
                2,
                call.client,
                None,
            ),
            B::ReleaseSourceFactory => (
                broker.source_factory,
                2,
                [broker.source_factory, 0, 0, 0, 0, 0],
                1,
                call.source,
                None,
            ),
            B::ReleaseClientFactory => (
                broker.client_factory,
                2,
                [broker.client_factory, 0, 0, 0, 0, 0],
                1,
                call.client,
                None,
            ),
            B::Publish | B::Failed(_) => unreachable!(),
        };
        let (sender, continuation) = (call.sender, call.continuation);
        if execution.apartment == ApartmentId32::Mta && execution.thread != sender {
            execution.thread = self.ensure_com_apartment_host(false)?;
        }
        let procedure = if let Some(gate) = gate {
            self.module_gates()?
                .iter()
                .find(|item| item.id == gate.id())
                .ok_or(HostError::Infrastructure)?
                .entry
        } else {
            self.com_guest_method(object, slot)?
        };
        self.queue_com_apartment_invocation(
            execution.thread,
            procedure,
            &args[..count],
            sender,
            continuation,
        )?;
        if broker.build == B::Connect {
            self.ole32.as_mut().unwrap().rpc.brokers[index]
                .as_mut()
                .unwrap()
                .connected = true;
        }
        let call = &mut self.ole32.as_mut().unwrap().marshaling.transfers[transfer];
        call.execution = execution;
        call.remote = Remote32::Queued;
        Ok(())
    }
    pub(in crate::process::windows::ole32) fn accept_registered_ps_result(
        &mut self,
        transfer: usize,
        index: usize,
        result: u32,
    ) -> Result<(), HostError> {
        let call = &self.ole32.as_ref().unwrap().marshaling.transfers[transfer];
        let scratch = call.scratch().address.0;
        let phase = self.ole32.as_ref().unwrap().rpc.brokers[index]
            .as_ref()
            .unwrap()
            .build;
        let cancelled = call.abandoned || call.cancelled;
        let next;
        if result & 0x80000000 == 0
            || matches!(
                phase,
                B::ReleaseSourceFactory | B::ReleaseClientFactory | B::Failed(_)
            )
        {
            let field = match phase {
                B::SourceFactory => Some((64, B::Stub)),
                B::Stub => Some((68, B::ClientFactory)),
                B::ClientFactory => Some((72, B::Proxy)),
                B::Proxy => Some((76, B::Connect)),
                _ => None,
            };
            if let Some((offset, following)) = field {
                let pointer = self.ole32_read_word(scratch + offset)?;
                if pointer == 0 {
                    return self.fail_registered_ps_transfer(
                        transfer,
                        index,
                        HResult32::UNEXPECTED.0,
                    );
                }
                let interface = if phase == B::Proxy {
                    self.ole32_read_word(scratch + 80)?
                } else {
                    0
                };
                let broker = self.ole32.as_mut().unwrap().rpc.brokers[index]
                    .as_mut()
                    .unwrap();
                match phase {
                    B::SourceFactory => broker.source_factory = pointer,
                    B::Stub => broker.stub = pointer,
                    B::ClientFactory => broker.client_factory = pointer,
                    B::Proxy => {
                        broker.proxy = pointer;
                        broker.interface = interface
                    }
                    _ => unreachable!(),
                }
                next = following;
                if phase == B::Proxy && interface == 0 {
                    return self.fail_registered_ps_transfer(
                        transfer,
                        index,
                        HResult32::UNEXPECTED.0,
                    );
                }
            } else {
                let broker = self.ole32.as_mut().unwrap().rpc.brokers[index]
                    .as_mut()
                    .unwrap();
                next = match phase {
                    B::Connect => {
                        broker.connected = true;
                        B::ReleaseSourceFactory
                    }
                    B::ReleaseSourceFactory => {
                        broker.source_factory = 0;
                        B::ReleaseClientFactory
                    }
                    B::ReleaseClientFactory => {
                        broker.client_factory = 0;
                        B::Publish
                    }
                    B::Failed(step) => {
                        match step {
                            0 => broker.interface = 0,
                            2 => broker.proxy = 0,
                            4 => broker.stub = 0,
                            5 => broker.source_factory = 0,
                            6 => broker.client_factory = 0,
                            7 => broker.source_object = 0,
                            _ => {}
                        };
                        B::Failed(step + 1)
                    }
                    B::Publish => unreachable!(),
                    _ => unreachable!(),
                };
            }
        } else {
            return self.fail_registered_ps_transfer(transfer, index, result);
        }
        self.ole32.as_mut().unwrap().rpc.brokers[index]
            .as_mut()
            .unwrap()
            .build = next;
        self.ole32.as_mut().unwrap().marshaling.transfers[transfer].remote = Remote32::Idle;
        if cancelled && !matches!(phase, B::Failed(_)) {
            return self.fail_registered_ps_transfer(transfer, index, 0x80010108);
        }
        self.advance_registered_ps_transfer(transfer, index)
    }
    pub(in crate::process::windows::ole32) fn fail_registered_ps_transfer(
        &mut self,
        transfer: usize,
        index: usize,
        result: u32,
    ) -> Result<(), HostError> {
        let runtime = self.ole32.as_mut().unwrap();
        let call = &mut runtime.marshaling.transfers[transfer];
        let broker = runtime.rpc.brokers[index].as_mut().unwrap();
        if matches!(call.source.apartment,ApartmentId32::Sta(thread) if call.dead_sta.contains(&thread))
        {
            broker.source_factory = 0;
            broker.stub = 0;
            broker.source_object = 0;
        }
        if matches!(call.client.apartment,ApartmentId32::Sta(thread) if call.dead_sta.contains(&thread))
        {
            broker.client_factory = 0;
            broker.proxy = 0;
            broker.interface = 0;
        }
        if !matches!(broker.build, B::Failed(_)) {
            broker.build = B::Failed(0);
            broker.result = result;
        }
        call.remote = Remote32::Idle;
        self.advance_registered_ps_transfer(transfer, index)
    }
    fn publish_registered_ps_transfer(
        &mut self,
        transfer: usize,
        index: usize,
    ) -> Result<(), HostError> {
        let runtime = self.ole32.as_ref().unwrap();
        let broker = runtime.rpc.brokers[index].as_ref().unwrap();
        let (controller, channel) = (broker.controller.unwrap(), broker.channel.unwrap());
        if runtime
            .com
            .references(controller)
            .map_err(super::calls::com_error)?
            < 2
            || runtime
                .com
                .references(channel)
                .map_err(super::calls::com_error)?
                < 2
        {
            return self.fail_registered_ps_transfer(transfer, index, HResult32::UNEXPECTED.0);
        }
        let receipt = runtime
            .com
            .prepare_lifetime_changes(self.memory()?, &[], &[controller, channel], &[], &[], &[])
            .map_err(super::calls::com_error)?;
        let runtime = self.ole32.as_mut().unwrap();
        runtime
            .com
            .commit_lifetime_changes(self.memory.as_mut().unwrap(), receipt)
            .map_err(super::calls::com_error)?;
        let broker = runtime.rpc.brokers[index].as_mut().unwrap();
        broker.owns_source = true;
        let call = &mut runtime.marshaling.transfers[transfer];
        call.references[0].pointer = 0;
        call.output = broker.interface;
        call.result = 0;
        call.remote = Remote32::Idle;
        call.phase = P::Release(0);
        self.advance_com_transfer(transfer)
    }
    fn advance_registered_ps_cleanup(
        &mut self,
        transfer: usize,
        index: usize,
        mut step: usize,
    ) -> Result<(), HostError> {
        loop {
            let runtime = self.ole32.as_ref().unwrap();
            let broker = *runtime.rpc.brokers[index].as_ref().unwrap();
            let (pointer, slot, execution) = match step {
                0 => (broker.interface, 2, broker.client),
                1 => (broker.proxy, 4, broker.client),
                2 => (broker.proxy, 2, broker.client),
                3 => (broker.stub, 4, broker.source),
                4 => (broker.stub, 2, broker.source),
                5 => (broker.source_factory, 2, broker.source),
                6 => (broker.client_factory, 2, broker.client),
                7 => (
                    if broker.owns_source {
                        broker.source_object
                    } else {
                        0
                    },
                    2,
                    broker.source,
                ),
                _ => break,
            };
            if pointer == 0 {
                step += 1;
                continue;
            }
            let call = &runtime.marshaling.transfers[transfer];
            let (sender, continuation) = (call.sender, call.continuation);
            let mut execution = execution;
            if matches!(
                execution.apartment,
                ApartmentId32::Mta | ApartmentId32::Neutral
            ) && (execution.thread != sender || call.abandoned)
            {
                execution.thread = self.ensure_com_apartment_host(false)?;
            }
            let procedure = self.com_guest_method(pointer, slot)?;
            self.queue_com_apartment_invocation(
                execution.thread,
                procedure,
                &[pointer],
                sender,
                continuation,
            )?;
            let runtime = self.ole32.as_mut().unwrap();
            runtime.rpc.brokers[index].as_mut().unwrap().build = B::Failed(step);
            let call = &mut runtime.marshaling.transfers[transfer];
            call.execution = execution;
            call.remote = Remote32::Queued;
            return Ok(());
        }
        let runtime = self.ole32.as_ref().unwrap();
        let broker = runtime.rpc.brokers[index].as_ref().unwrap();
        let mut release = [crate::memory::GuestAddress(0); 2];
        let mut count = 0;
        for (pointer, live) in [
            (broker.controller, broker.controller_live),
            (broker.channel, broker.channel_live),
        ] {
            if live {
                release[count] = pointer.unwrap();
                count += 1;
            }
        }
        let receipt = runtime
            .com
            .prepare_lifetime_changes(self.memory()?, &[], &release[..count], &[], &[], &[])
            .map_err(super::calls::com_error)?;
        let runtime = self.ole32.as_mut().unwrap();
        let destroyed = runtime
            .com
            .commit_lifetime_changes(self.memory.as_mut().unwrap(), receipt)
            .map_err(super::calls::com_error)?;
        runtime.rpc.record_destroyed_identities(&destroyed);
        let broker = *runtime.rpc.brokers[index].as_ref().unwrap();
        runtime.rpc.brokers[index].as_mut().unwrap().owns_source = false;
        runtime.rpc.retire_disconnected_broker(index);
        let call = &mut runtime.marshaling.transfers[transfer];
        call.result = broker.result;
        call.output = 0;
        call.remote = Remote32::Idle;
        call.phase = P::Release(0);
        self.advance_com_transfer(transfer)
    }
}
