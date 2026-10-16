use super::context::{STREAM_PAYLOAD, StreamEntry32, stream_error};
use crate::{
    memory::{AddressSpace, GuestAddress},
    process::{
        EngineInstance, HostError,
        windows::{
            objects::{GuestComCreation32, InterfaceLayout32},
            ole32::{Ole32Runtime32, memory::com_error},
        },
    },
    windows::{
        com::IID_IUNKNOWN,
        ole32::{HResult32, IID_ISEQUENTIALSTREAM, IID_ISTREAM, stream::StreamId32},
    },
};

impl EngineInstance {
    pub(in crate::process::windows::ole32) fn create_com_stream(
        &mut self,
        maximum: usize,
        bytes: &[u8],
    ) -> Result<GuestAddress, HostError> {
        let gates = self.module_gates()?;
        let runtime = self.ole32.as_mut().ok_or(HostError::InvalidRequest)?;
        runtime.initialize_stream_methods(&gates)?;
        runtime
            .create_stream(
                self.memory.as_mut().ok_or(HostError::Closed)?,
                maximum,
                bytes,
            )
            .map_err(|error| match error {
                HResult32::OUT_OF_MEMORY => HostError::Infrastructure,
                _ => HostError::InvalidRequest,
            })
    }
}
impl Ole32Runtime32 {
    pub(in crate::process::windows::ole32) fn create_stream(
        &mut self,
        memory: &mut AddressSpace,
        maximum: usize,
        bytes: &[u8],
    ) -> Result<GuestAddress, HResult32> {
        let prepared = self
            .streams
            .store
            .prepare_stream(maximum, bytes)
            .map_err(stream_error)?;
        let index = self
            .streams
            .entries
            .iter()
            .position(Option::is_none)
            .ok_or(HResult32::OUT_OF_MEMORY)?;
        let pointer = self.publish_stream_identity(memory, index, None)?;
        let id = self.streams.store.commit_stream(prepared);
        self.streams.entries[index] = Some(StreamEntry32 { pointer, id });
        Ok(pointer)
    }
    pub(super) fn clone_stream(
        &mut self,
        memory: &mut AddressSpace,
        source: StreamId32,
        output: u32,
    ) -> Result<u32, HResult32> {
        let prepared = self
            .streams
            .store
            .prepare_clone(source)
            .map_err(stream_error)?;
        let index = self
            .streams
            .entries
            .iter()
            .position(Option::is_none)
            .ok_or(HResult32::OUT_OF_MEMORY)?;
        let pointer = self.publish_stream_identity(memory, index, Some(output))?;
        let id = self.streams.store.commit_clone(prepared);
        self.streams.entries[index] = Some(StreamEntry32 { pointer, id });
        Ok(0)
    }
    fn publish_stream_identity(
        &mut self,
        memory: &mut AddressSpace,
        index: usize,
        output: Option<u32>,
    ) -> Result<GuestAddress, HResult32> {
        let methods = self.streams.methods;
        let identity = self
            .com
            .create_with_lifetime(
                memory,
                STREAM_PAYLOAD | index as u64,
                &[
                    InterfaceLayout32 {
                        iid: IID_IUNKNOWN,
                        methods: &methods[..3],
                    },
                    InterfaceLayout32 {
                        iid: IID_ISEQUENTIALSTREAM,
                        methods: &methods[..5],
                    },
                    InterfaceLayout32 {
                        iid: IID_ISTREAM,
                        methods: &methods,
                    },
                ],
                GuestComCreation32 {
                    output: output.map(|out| (GuestAddress(out), IID_ISTREAM)),
                    ..Default::default()
                },
            )
            .map_err(com_error)?;
        self.com
            .query_target(memory, identity.identity_pointer, IID_ISTREAM)
            .map_err(com_error)
    }
}
