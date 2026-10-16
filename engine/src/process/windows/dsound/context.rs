use super::{
    DSoundConfig32, DSoundError32,
    locks::LockRegistry32,
    objects::{Object32, ObjectKind32},
};
use crate::{
    audio::dsound::{HResult, InterfaceKind, MethodGate},
    cpu::dbt::GateSpec,
    memory::GuestAddress,
    process::windows::objects::GuestComObjects32,
};

pub(in crate::process) struct DSoundRuntime32 {
    pub(super) config: DSoundConfig32,
    pub(super) com: GuestComObjects32,
    pub(super) records: Vec<Option<Object32>>,
    pub(super) methods: Vec<Vec<GuestAddress>>,
    pub(super) locks: LockRegistry32,
    pub(super) enumerations: Vec<super::enumeration::PendingEnumeration32>,
    pub(super) enumeration_data: Option<super::enumeration::EnumerationData32>,
    pub(super) sequence: u64,
    pub(super) signals: Vec<(crate::windows::synchronization::EventBinding32, u64)>,
    pub(super) playback_lost: bool,
    pub(super) capture_aec: crate::audio::capture_effects::AecParameters,
    pub(super) capture_noise: bool,
}
impl DSoundRuntime32 {
    pub(in crate::process) fn new(config: DSoundConfig32) -> Result<Self, DSoundError32> {
        if config.max_objects < 4
            || config.max_objects > 4096
            || config.max_locks == 0
            || config.max_locks > 4096
            || config.max_notifications > 65_536
            || config.max_capture_bytes > 256 * 1024 * 1024
            || config.objects.first == 0
            || config.locks.first == 0
            || config.objects.first < config.locks.first + config.locks.count
                && config.locks.first < config.objects.first + config.objects.count
        {
            return Err(DSoundError32::InvalidConfiguration);
        }
        let com = GuestComObjects32::new(
            config.objects,
            config.max_objects as usize,
            config.max_objects as usize * 4,
            InterfaceKind::ALL.len() + 2,
        )
        .map_err(|_| DSoundError32::InvalidConfiguration)?;
        let mut records = Vec::new();
        records
            .try_reserve_exact(config.max_objects as usize)
            .map_err(|_| DSoundError32::Allocation)?;
        records.resize_with(config.max_objects as usize, || None);
        let mut enumerations = Vec::new();
        enumerations
            .try_reserve_exact(33)
            .map_err(|_| DSoundError32::Allocation)?;
        let mut signals = Vec::new();
        signals
            .try_reserve_exact(config.max_notifications as usize)
            .map_err(|_| DSoundError32::Allocation)?;
        Ok(Self {
            config,
            com,
            records,
            methods: Vec::new(),
            locks: LockRegistry32::new(config.locks, config.max_locks)?,
            enumerations,
            enumeration_data: None,
            sequence: 0,
            signals,
            playback_lost: false,
            capture_aec: Default::default(),
            capture_noise: true,
        })
    }
    pub(in crate::process::windows::dsound) fn initialize_methods(
        &mut self,
        gates: &[GateSpec],
    ) -> Result<(), HResult> {
        if !self.methods.is_empty() {
            return Ok(());
        }
        let mut methods = Vec::new();
        methods
            .try_reserve_exact(InterfaceKind::ALL.len())
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        for interface in InterfaceKind::ALL {
            let mut table = Vec::new();
            table
                .try_reserve_exact(interface.methods().len())
                .map_err(|_| HResult::OUT_OF_MEMORY)?;
            for slot in 0..interface.methods().len() {
                let id = MethodGate {
                    interface,
                    slot: slot as u8,
                }
                .id();
                table.push(
                    gates
                        .iter()
                        .find(|gate| gate.id == id)
                        .ok_or(HResult::INVALID_PARAM)?
                        .entry,
                );
            }
            methods.push(table);
        }
        self.methods = methods;
        Ok(())
    }
    pub(in crate::process::windows::dsound) fn vacant(&self) -> Result<usize, HResult> {
        self.records
            .iter()
            .position(Option::is_none)
            .ok_or(HResult::OUT_OF_MEMORY)
    }
    pub(in crate::process::windows::dsound) fn slot(
        &self,
        pointer: GuestAddress,
    ) -> Result<usize, HResult> {
        let identity = self.com.lookup(pointer).map_err(HResult::from)?;
        let slot = usize::try_from(identity.payload).map_err(|_| HResult::INVALID_PARAM)?;
        self.records
            .get(slot)
            .and_then(Option::as_ref)
            .filter(|object| object.pointer == identity.identity_pointer)
            .ok_or(HResult::INVALID_PARAM)?;
        Ok(slot)
    }
    pub(in crate::process::windows::dsound) fn object(
        &self,
        pointer: GuestAddress,
    ) -> Result<&Object32, HResult> {
        Ok(self.records[self.slot(pointer)?].as_ref().unwrap())
    }
    pub(in crate::process::windows::dsound) fn device(
        &self,
        pointer: GuestAddress,
    ) -> Result<&super::objects::Device32, HResult> {
        match &self.object(pointer)?.kind {
            ObjectKind32::Device(device) => Ok(device),
            _ => Err(HResult::INVALID_PARAM),
        }
    }
}
impl From<crate::process::windows::objects::GuestComError32> for HResult {
    fn from(error: crate::process::windows::objects::GuestComError32) -> Self {
        use crate::process::windows::objects::GuestComError32 as E;
        match error {
            E::Allocation | E::Capacity => Self::OUT_OF_MEMORY,
            E::Com(crate::windows::com::ComError32::NoInterface) => Self::NO_INTERFACE,
            _ => Self::INVALID_PARAM,
        }
    }
}
impl From<crate::memory::MemoryError> for HResult {
    fn from(_: crate::memory::MemoryError) -> Self {
        Self::INVALID_PARAM
    }
}

impl DSoundRuntime32 {
    pub(in crate::process) fn guest_reservations(&self) -> [crate::memory::PageRange; 2] {
        [self.config.objects, self.config.locks]
    }
}
