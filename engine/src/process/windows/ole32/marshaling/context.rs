use crate::{
    memory::GuestAddress,
    process::{
        calls::call::PendingOwner,
        windows::callbacks::{GuestCallbackScratch32, WindowsCallback32},
    },
    windows::{
        CallFrame32,
        com::InterfaceId,
        ole32::{ApartmentId32, FactoryMethod32},
    },
};

pub(in crate::process::windows::ole32) const PROXY_PAYLOAD: u64 = 1 << 63;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::process::windows::ole32) struct ExecutionApartment32 {
    pub apartment: ApartmentId32,
    pub thread: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::process::windows::ole32) enum Remote32 {
    #[default]
    Idle,
    Queued,
    Ready(u32),
}

#[derive(Clone, Copy)]
pub(in crate::process::windows::ole32) struct Proxy32 {
    pub client: ApartmentId32,
    pub execution: ExecutionApartment32,
    pub object: u32,
    pub factory: Option<u32>,
    pub factory_extra: bool,
    pub server: Option<u64>,
    pub pointer: GuestAddress,
    pub unknown: GuestAddress,
    pub factory_view: GuestAddress,
    pub disconnected: bool,
    pub owned: [u32; 2],
    pub local_alive: bool,
    pub rundown: Option<Rundown32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProxyPhase32 {
    Query { requested: InterfaceId },
    Create { requested: InterfaceId },
    Lock,
    Release { next: usize },
    Publish { result: u32 },
    Transfer { result: u32 },
}

pub(super) struct ProxyCall32 {
    pub continuation: u64,
    pub thread: u32,
    pub owner: PendingOwner,
    pub token: u32,
    pub frame: CallFrame32,
    pub method: FactoryMethod32,
    pub proxy: usize,
    pub execution: ExecutionApartment32,
    pub out: u32,
    pub scratch: GuestCallbackScratch32,
    pub phase: ProxyPhase32,
    pub remote: Remote32,
    pub abandoned: bool,
    pub cleanup: Option<Cleanup32>,
}

pub(in crate::process::windows::ole32) struct MarshalingRuntime32 {
    pub proxies: Vec<Option<Proxy32>>,
    pub(super) calls: Vec<ProxyCall32>,
    pub activations: Vec<RemoteActivation32>,
    pub transfers: Vec<super::transfer::TransferCall32>,
}

pub(in crate::process::windows::ole32) struct RemoteActivation32 {
    pub continuation: u64,
    pub client: ApartmentId32,
    pub execution: ExecutionApartment32,
    pub remote: Remote32,
    pub next: Option<WindowsCallback32>,
    pub abandoned: bool,
    pub cleanup: Option<Cleanup32>,
}

#[derive(Clone, Copy)]
pub(in crate::process::windows::ole32) struct Cleanup32 {
    pub pointers: [u32; 2],
    pub count: usize,
    pub next: usize,
}

#[derive(Clone, Copy)]
pub(in crate::process::windows::ole32) struct Rundown32 {
    pub continuation: u64,
    pub sender: u32,
    pub next: usize,
    pub remote: Remote32,
}

impl MarshalingRuntime32 {
    pub(in crate::process::windows::ole32) fn new(
        objects: usize,
    ) -> Result<Self, super::super::Ole32Error32> {
        let mut proxies = Vec::new();
        proxies
            .try_reserve_exact(objects)
            .map_err(|_| super::super::Ole32Error32::Allocation)?;
        proxies.resize_with(objects, || None);
        let mut activations = Vec::new();
        activations
            .try_reserve_exact(64)
            .map_err(|_| super::super::Ole32Error32::Allocation)?;
        Ok(Self {
            proxies,
            calls: Vec::new(),
            activations,
            transfers: Vec::new(),
        })
    }
}
