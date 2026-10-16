use crate::{
    memory::GuestAddress,
    process::{
        HostError,
        calls::call::PendingOwner,
        windows::{
            callbacks::GuestCallbackScratch32,
            ole32::{
                Ole32Error32,
                marshaling::{ExecutionApartment32, Remote32},
            },
        },
    },
    windows::{
        CallFrame32,
        com::InterfaceId,
        ole32::{ApartmentId32, stream::BufferId32},
    },
};
pub(super) const CONTROLLER_PAYLOAD: u64 = 1 << 60;
pub(super) const CHANNEL_PAYLOAD: u64 = 1 << 61;
pub(super) struct RpcBuffer32 {
    pub id: BufferId32,
    pub channel: usize,
    pub bytes: u32,
    pub scratch: GuestCallbackScratch32,
    pub owner: Option<u64>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BuildPhase32 {
    SourceFactory,
    Stub,
    ClientFactory,
    Proxy,
    Connect,
    ReleaseSourceFactory,
    ReleaseClientFactory,
    Publish,
    Failed(usize),
}
#[derive(Clone, Copy)]
pub(super) struct RpcRundown32 {
    pub continuation: u64,
    pub sender: u32,
    pub next: usize,
    pub remote: Remote32,
    pub execution: ExecutionApartment32,
    pub retiring: bool,
}
#[derive(Clone, Copy)]
pub(super) struct Broker32 {
    pub source: ExecutionApartment32,
    pub client: ExecutionApartment32,
    pub requested: InterfaceId,
    pub source_object: u32,
    pub owns_source: bool,
    pub result: u32,
    pub source_factory: u32,
    pub stub: u32,
    pub client_factory: u32,
    pub proxy: u32,
    pub interface: u32,
    pub controller: Option<GuestAddress>,
    pub channel: Option<GuestAddress>,
    pub controller_live: bool,
    pub channel_live: bool,
    pub proxy_channel_references: u32,
    pub connected: bool,
    pub build: BuildPhase32,
    pub rundown: Option<RpcRundown32>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RpcPhase32 {
    Invoke,
    Query(InterfaceId),
    Transfer,
    ReleaseQuery,
    Retire,
    Cleanup(usize),
}
pub(super) struct RpcCall32 {
    pub continuation: u64,
    pub sender: u32,
    pub owner: PendingOwner,
    pub token: u32,
    pub frame: CallFrame32,
    pub broker: usize,
    pub execution: ExecutionApartment32,
    pub remote: Remote32,
    pub phase: RpcPhase32,
    pub message: u32,
    pub status: u32,
    pub scratch: Option<GuestCallbackScratch32>,
    pub request_buffer: u32,
    pub abandoned: bool,
    pub cancelled: bool,
    pub returned: u32,
    pub request: Option<crate::windows::ole32::rpc::RpcMessage32>,
    pub request_lease: Option<BufferId32>,
    pub held_channel: Option<GuestAddress>,
}
pub(in crate::process::windows::ole32) struct RpcRuntime32 {
    pub(super) brokers: Vec<Option<Broker32>>,
    pub(super) buffers: Vec<RpcBuffer32>,
    pub(super) calls: Vec<RpcCall32>,
    pub(super) methods: [GuestAddress; 8],
}
impl RpcRuntime32 {
    #[cfg(test)]
    pub(in crate::process::windows::ole32) fn live_brokers(&self) -> usize {
        self.brokers.iter().flatten().count()
    }
    pub(in crate::process::windows::ole32) fn new(objects: usize) -> Result<Self, Ole32Error32> {
        let mut brokers = Vec::new();
        brokers
            .try_reserve_exact(objects)
            .map_err(|_| Ole32Error32::Allocation)?;
        brokers.resize_with(objects, || None);
        let mut buffers = Vec::new();
        buffers
            .try_reserve_exact(
                objects
                    .checked_mul(4)
                    .ok_or(Ole32Error32::InvalidConfiguration)?,
            )
            .map_err(|_| Ole32Error32::Allocation)?;
        let mut calls = Vec::new();
        calls
            .try_reserve_exact(64)
            .map_err(|_| Ole32Error32::Allocation)?;
        Ok(Self {
            brokers,
            buffers,
            calls,
            methods: [GuestAddress(0); 8],
        })
    }
    pub(super) fn buffer(
        &self,
        channel: usize,
        pointer: u32,
        bytes: u32,
    ) -> Result<usize, HostError> {
        self.buffers
            .iter()
            .position(|buffer| {
                buffer.channel == channel
                    && buffer.scratch.address.0 == pointer
                    && bytes <= buffer.bytes
            })
            .ok_or(HostError::InvalidRequest)
    }
    pub(super) fn apartment_matches(&self, index: usize, apartment: ApartmentId32) -> bool {
        self.brokers[index]
            .as_ref()
            .is_some_and(|broker| broker.client.apartment == apartment)
    }

    pub(super) fn packet(
        &self,
        channel: usize,
        pointer: u32,
        bytes: u32,
        owner: Option<u64>,
    ) -> Result<usize, HostError> {
        let index = self.buffer(channel, pointer, bytes)?;
        if self.buffers[index].owner.is_some() && self.buffers[index].owner != owner {
            return Err(HostError::InvalidRequest);
        }
        Ok(index)
    }
}
