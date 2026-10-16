use crate::{
    memory::{GuestAddress, PageRange},
    process::{
        calls::call::PendingOwner,
        windows::callbacks::{GuestCallbackScratch32, WindowsCallback32, WindowsProviderOutcome32},
    },
    windows::{
        CallFrame32,
        com::ServerThreadingModel32,
        ole32::{HResult32, Ole32Api32},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::process::windows::ole32) enum Phase32 {
    Load,
    Loading,
    Resolve { handle: u32 },
    Resolving,
    Factory,
    Create { factory: u32, release: u32 },
    Release { result: u32 },
    Finish { result: u32 },
    UnloadNext { after: u64, delay: u32 },
    ResolveUnload { lease: ServerUse32, delay: u32 },
    CanUnload { lease: ServerUse32, delay: u32 },
    Unloading { lease: ServerUse32, delay: u32 },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::process::windows::ole32) struct ServerUse32 {
    pub server: u64,
    pub handle: u32,
    pub activity: u64,
}
pub(in crate::process) enum Ole32CallbackOutcome32 {
    Complete(u32),
    Callback(WindowsCallback32),
    LoaderPending,
}
impl From<WindowsProviderOutcome32> for Ole32CallbackOutcome32 {
    fn from(value: WindowsProviderOutcome32) -> Self {
        match value {
            WindowsProviderOutcome32::Return(result) => Self::Complete(result),
            WindowsProviderOutcome32::Callback(request) => Self::Callback(request),
        }
    }
}
pub(in crate::process::windows::ole32) struct PendingActivation32 {
    pub continuation: u64,
    pub thread: u32,
    pub owner: PendingOwner,
    pub token: u32,
    pub frame: CallFrame32,
    pub api: Ole32Api32,
    pub out: u32,
    pub outer: u32,
    pub path: String,
    pub scratch: GuestCallbackScratch32,
    pub phase: Phase32,
    pub threading: ServerThreadingModel32,
    pub server_threads: Vec<u32>,
    pub server: Option<u64>,
}
pub(in crate::process::windows::ole32) struct Server32 {
    pub id: u64,
    pub path: String,
    pub handle: u32,
    pub threading: ServerThreadingModel32,
    pub threads: Vec<u32>,
    pub mta_lease: bool,
    pub neutral_lease: bool,
    pub unused_deadline: Option<u64>,
    pub activity: u64,
}
impl Server32 {
    pub(super) fn closes_with(
        &self,
        apartments: &crate::windows::ole32::Apartments32,
        thread: u32,
    ) -> bool {
        (self.threads.contains(&thread)
            || (self.mta_lease && apartments.is_single_threaded(thread) == Ok(false))
            || self.neutral_lease)
            && self.threads.iter().all(|owner| *owner == thread)
            && (!self.mta_lease || !apartments.has_other_user_mta(thread))
            && (!self.neutral_lease || !apartments.has_other_user(thread))
    }
    pub(super) fn lease(&self) -> ServerUse32 {
        ServerUse32 {
            server: self.id,
            handle: self.handle,
            activity: self.activity,
        }
    }
}
#[derive(Default)]
pub(in crate::process::windows::ole32) struct ActivationRuntime32 {
    pub pending: Vec<PendingActivation32>,
    pub servers: Vec<Server32>,
    pub next: u64,
}
impl ActivationRuntime32 {
    pub(super) fn load_ready(&self, slot: usize) -> bool {
        let pending = &self.pending[slot];
        if self.pending.iter().enumerate().any(|(index, other)| {
            index != slot
                && other.phase == Phase32::Loading
                && other.path.eq_ignore_ascii_case(&pending.path)
        }) {
            return false;
        }
        self.servers
            .iter()
            .find(|server| server.path.eq_ignore_ascii_case(&pending.path))
            .is_none_or(|server| !self.unloading(server.handle, slot))
    }

    pub(super) fn unloading(&self, handle: u32, except: usize) -> bool {
        self.pending.iter().enumerate().any(|(index, pending)| {
            index != except
                && matches!(pending.phase, Phase32::Unloading { lease, .. } if lease.handle == handle)
        })
    }

    pub(super) fn server_held(&self, handle: u32, except: usize) -> bool {
        self.pending.iter().enumerate().any(|(index, pending)| {
            index != except
                && pending.server.is_some_and(|id| {
                    self.servers
                        .iter()
                        .any(|server| server.id == id && server.handle == handle)
                })
        })
    }

    pub(super) fn unload_reserved(&self, handle: u32, except: usize) -> bool {
        self.pending.iter().enumerate().any(|(index, pending)| {
            index != except
                && matches!(pending.phase,
                    Phase32::ResolveUnload {lease,..}
                    |Phase32::CanUnload {lease,..}
                    |Phase32::Unloading {lease,..} if lease.handle == handle)
        })
    }

    pub(super) fn release_server(&mut self, lease: ServerUse32) {
        self.servers
            .retain(|server| server.id != lease.server || server.handle != lease.handle);
    }

    pub(in crate::process::windows::ole32) fn prepare(&mut self) -> Result<u64, HResult32> {
        if self.pending.len() >= 64 {
            return Err(HResult32::OUT_OF_MEMORY);
        }
        self.pending
            .try_reserve(1)
            .map_err(|_| HResult32::OUT_OF_MEMORY)?;
        self.servers
            .try_reserve((self.pending.len() + 1).min(256 - self.servers.len()))
            .map_err(|_| HResult32::OUT_OF_MEMORY)?;
        self.next.checked_add(1).ok_or(HResult32::OUT_OF_MEMORY)
    }
    pub(in crate::process) fn scratch_reservation() -> PageRange {
        PageRange::new(GuestAddress(0x69e00000), 64).expect("bounded COM activation scratch")
    }
}
