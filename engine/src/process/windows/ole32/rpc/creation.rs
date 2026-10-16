use super::context::{Broker32, BuildPhase32, CHANNEL_PAYLOAD, CONTROLLER_PAYLOAD};
use crate::{
    memory::GuestAddress,
    process::{
        EngineInstance, HostError,
        windows::{objects::InterfaceLayout32, ole32::marshaling::transfer::TransferPhase32},
    },
    windows::{
        com::IID_IUNKNOWN,
        ole32::{
            RpcChannelMethod32,
            rpc::{IID_IPSFACTORYBUFFER, IID_IRPCCHANNELBUFFER},
        },
    },
};
impl EngineInstance {
    pub(in crate::process::windows::ole32) fn begin_registered_com_transfer(
        &mut self,
        transfer: usize,
        class: crate::windows::com::InterfaceId,
    ) -> Result<(), HostError> {
        let call = &self.ole32.as_ref().unwrap().marshaling.transfers[transfer];
        let index = self
            .ole32
            .as_ref()
            .unwrap()
            .rpc
            .brokers
            .iter()
            .position(Option::is_none)
            .ok_or(HostError::Infrastructure)?;
        let scratch = call.scratch().address.0;
        let broker = Broker32 {
            source: call.source,
            client: call.client,
            requested: call.requested,
            source_object: call.references[0].pointer,
            owns_source: false,
            result: 0,
            source_factory: 0,
            stub: 0,
            client_factory: 0,
            proxy: 0,
            interface: 0,
            controller: None,
            channel: None,
            controller_live: false,
            channel_live: false,
            proxy_channel_references: 0,
            connected: false,
            build: BuildPhase32::SourceFactory,
            rundown: None,
        };
        self.memory
            .as_mut()
            .unwrap()
            .write_spans(&[
                (GuestAddress(scratch + 32), &class.0),
                (GuestAddress(scratch + 16), &IID_IPSFACTORYBUFFER.0),
            ])
            .map_err(HostError::Memory)?;
        let runtime = self.ole32.as_mut().unwrap();
        runtime.rpc.brokers[index] = Some(broker);
        let call = &mut runtime.marshaling.transfers[transfer];
        call.phase = TransferPhase32::ProxyStub(index);
        call.remote = super::super::marshaling::Remote32::Idle;
        self.advance_com_transfer(transfer)
    }
    pub(super) fn prepare_rpc_identities(&mut self, index: usize) -> Result<(), HostError> {
        let gates = self.module_gates()?;
        let runtime = self.ole32.as_mut().ok_or(HostError::InvalidRequest)?;
        runtime
            .initialize_methods(&gates)
            .map_err(|_| HostError::Infrastructure)?;
        for (slot, method) in runtime.rpc.methods.iter_mut().enumerate() {
            *method = gates
                .iter()
                .find(|gate| gate.id == RpcChannelMethod32(slot as u8).id())
                .ok_or(HostError::Infrastructure)?
                .entry;
        }
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        if runtime.rpc.brokers[index]
            .as_ref()
            .unwrap()
            .controller
            .is_none()
        {
            let methods = runtime.methods;
            let identity = runtime
                .com
                .create(
                    memory,
                    CONTROLLER_PAYLOAD | index as u64,
                    &[InterfaceLayout32 {
                        iid: IID_IUNKNOWN,
                        methods: &methods[..3],
                    }],
                )
                .map_err(super::calls::com_error)?;
            let broker = runtime.rpc.brokers[index].as_mut().unwrap();
            broker.controller = Some(identity.identity_pointer);
            broker.controller_live = true;
        }
        if runtime.rpc.brokers[index]
            .as_ref()
            .unwrap()
            .channel
            .is_none()
        {
            let methods = runtime.rpc.methods;
            let identity = runtime
                .com
                .create(
                    memory,
                    CHANNEL_PAYLOAD | index as u64,
                    &[
                        InterfaceLayout32 {
                            iid: IID_IUNKNOWN,
                            methods: &methods[..3],
                        },
                        InterfaceLayout32 {
                            iid: IID_IRPCCHANNELBUFFER,
                            methods: &methods,
                        },
                    ],
                )
                .map_err(super::calls::com_error)?;
            let pointer = runtime
                .com
                .query_target(memory, identity.identity_pointer, IID_IRPCCHANNELBUFFER)
                .map_err(super::calls::com_error)?;
            let broker = runtime.rpc.brokers[index].as_mut().unwrap();
            broker.channel = Some(pointer);
            broker.channel_live = true;
        }
        Ok(())
    }
}
