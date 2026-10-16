use crate::{
    memory::{AddressSpace, GuestAddress},
    process::{
        HostError,
        windows::ole32::{Ole32Error32, Ole32Runtime32},
    },
    windows::ole32::{
        HResult32, StreamMethod32,
        stream::{StreamError32, StreamId32, StreamLimits32, StreamStore32},
    },
};

pub(super) const STREAM_PAYLOAD: u64 = 1 << 62;
#[derive(Clone, Copy)]
pub(super) struct StreamEntry32 {
    pub pointer: GuestAddress,
    pub id: StreamId32,
}
pub(in crate::process::windows::ole32) struct StreamRuntime32 {
    pub(in crate::process::windows::ole32) store: StreamStore32,
    pub(super) entries: Vec<Option<StreamEntry32>>,
    pub(super) methods: [GuestAddress; 14],
    pub(in crate::process::windows::ole32) copy_limit: usize,
    pub(in crate::process::windows::ole32) copies: Vec<super::copy::StreamCopy32>,
}
impl StreamRuntime32 {
    pub(in crate::process::windows::ole32) fn buffer_reservation() -> crate::memory::PageRange {
        crate::memory::PageRange::new(GuestAddress(0x67000000), 4096)
            .expect("bounded in-process RPC buffer reservation")
    }
    pub(in crate::process::windows::ole32) fn new(objects: usize) -> Result<Self, Ole32Error32> {
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(objects)
            .map_err(|_| Ole32Error32::Allocation)?;
        entries.resize_with(objects, || None);
        let limits =
            StreamLimits32::for_objects(objects).map_err(|_| Ole32Error32::InvalidConfiguration)?;
        let mut copies = Vec::new();
        copies
            .try_reserve_exact(objects.min(64))
            .map_err(|_| Ole32Error32::Allocation)?;
        Ok(Self {
            copies,
            copy_limit: objects.min(64),
            store: StreamStore32::new(limits).map_err(|_| Ole32Error32::Allocation)?,
            entries,
            methods: [GuestAddress(0); 14],
        })
    }
}
impl Ole32Runtime32 {
    pub(in crate::process::windows::ole32) fn stream_index(
        &self,
        memory: &AddressSpace,
        pointer: u32,
    ) -> Result<usize, HostError> {
        let identity = self
            .com
            .preflight(memory, GuestAddress(pointer))
            .map_err(|error| match error {
                crate::process::windows::objects::GuestComError32::Memory(error) => {
                    HostError::Memory(error)
                }
                _ => HostError::InvalidRequest,
            })?;
        if identity.payload & !(STREAM_PAYLOAD - 1) != STREAM_PAYLOAD {
            return Err(HostError::InvalidRequest);
        }
        let index = (identity.payload & (STREAM_PAYLOAD - 1)) as usize;
        self.streams
            .entries
            .get(index)
            .and_then(Option::as_ref)
            .ok_or(HostError::InvalidRequest)?;
        Ok(index)
    }
    pub(in crate::process::windows::ole32) fn initialize_stream_methods(
        &mut self,
        gates: &[crate::cpu::dbt::GateSpec],
    ) -> Result<(), HostError> {
        for (slot, method) in self.streams.methods.iter_mut().enumerate() {
            *method = gates
                .iter()
                .find(|gate| gate.id == StreamMethod32(slot as u8).id())
                .ok_or(HostError::Infrastructure)?
                .entry;
        }
        Ok(())
    }
}
pub(super) fn stream_error(error: StreamError32) -> HResult32 {
    HResult32(match error {
        StreamError32::InvalidFunction => 0x8003_0001,
        StreamError32::InvalidHandle => 0x8003_0006,
        StreamError32::InvalidParameter => 0x8003_0057,
        StreamError32::OutOfMemory => 0x8003_0008,
        StreamError32::MediumFull => 0x8003_0070,
        StreamError32::Stale => 0x8000_ffff,
    })
}
