use super::{
    DirectInputConfig32, DirectInputError32,
    objects::{Device32, Object32, ObjectKind32},
};
use crate::{
    cpu::dbt::GateSpec,
    input::dinput::{
        ControllerDescriptor32, DirectInputResult32 as H, InterfaceKind32, MethodGate,
    },
    memory::GuestAddress,
    process::windows::objects::GuestComObjects32,
};
pub(in crate::process) struct DirectInputRuntime32 {
    pub(super) config: DirectInputConfig32,
    pub(super) com: GuestComObjects32,
    pub(super) records: Vec<Option<Object32>>,
    pub(super) methods: Vec<Vec<GuestAddress>>,
    pub(super) controllers: [Option<ControllerDescriptor32>; 8],
    pub(super) used_devices: [bool; 10],
    pub(super) owned_devices: [bool; 10],
    pub(super) pending: Vec<super::enumeration::PendingEnumeration32>,
    pub(super) sequence: u64,
    pub(super) signals: Vec<crate::windows::synchronization::EventBinding32>,
}
impl DirectInputRuntime32 {
    pub(in crate::process) fn new(config: DirectInputConfig32) -> Result<Self, DirectInputError32> {
        if config.objects.first == 0
            || config.scratch.first == 0
            || config.max_objects < 2
            || config.max_objects > 4096
            || config.max_devices == 0
            || config.max_devices > 256
            || config.max_devices >= config.max_objects
            || config.max_buffer_events == 0
            || config.max_buffer_events > 65536
            || config.objects.first < config.scratch.first + config.scratch.count
                && config.scratch.first < config.objects.first + config.objects.count
        {
            return Err(DirectInputError32::InvalidConfiguration);
        }
        let com = GuestComObjects32::new(
            config.objects,
            config.max_objects as usize,
            config.max_objects as usize * 6,
            16,
        )
        .map_err(|_| DirectInputError32::InvalidConfiguration)?;
        let mut records = Vec::new();
        records
            .try_reserve_exact(config.max_objects as usize)
            .map_err(|_| DirectInputError32::Allocation)?;
        records.resize_with(config.max_objects as usize, || None);
        let mut methods = Vec::new();
        methods
            .try_reserve_exact(16)
            .map_err(|_| DirectInputError32::Allocation)?;
        methods.resize_with(16, Vec::new);
        let mut pending = Vec::new();
        pending
            .try_reserve_exact(33)
            .map_err(|_| DirectInputError32::Allocation)?;
        let mut signals = Vec::new();
        signals
            .try_reserve_exact(config.max_devices as usize)
            .map_err(|_| DirectInputError32::Allocation)?;
        Ok(Self {
            config,
            com,
            records,
            methods,
            controllers: std::array::from_fn(|_| None),
            used_devices: [false; 10],
            owned_devices: [false; 10],
            pending,
            sequence: 0,
            signals,
        })
    }
    pub(super) fn initialize_methods(
        &mut self,
        gates: &[GateSpec],
        version8: bool,
    ) -> Result<(), H> {
        for interface in InterfaceKind32::ALL
            .into_iter()
            .filter(|kind| (kind.generation() == 8) == version8)
        {
            let table = &mut self.methods[interface as usize - 1];
            if !table.is_empty() {
                continue;
            }
            let mut next = Vec::new();
            next.try_reserve_exact(interface.words().len())
                .map_err(|_| H::OUT_OF_MEMORY)?;
            for slot in 0..interface.words().len() {
                let id = MethodGate {
                    interface,
                    slot: slot as u8,
                }
                .id();
                next.push(
                    gates
                        .iter()
                        .find(|gate| gate.id == id)
                        .ok_or(H::NOT_INITIALIZED)?
                        .entry,
                );
            }
            *table = next;
        }
        Ok(())
    }
    pub(super) fn slot(&self, pointer: GuestAddress) -> Result<usize, H> {
        let identity = self.com.lookup(pointer)?;
        let slot = usize::try_from(identity.payload).map_err(|_| H::INVALID_PARAM)?;
        self.records
            .get(slot)
            .and_then(Option::as_ref)
            .filter(|record| record.pointer == identity.identity_pointer)
            .ok_or(H::INVALID_PARAM)?;
        Ok(slot)
    }
    pub(super) fn object(&self, pointer: GuestAddress) -> Result<&Object32, H> {
        Ok(self.records[self.slot(pointer)?].as_ref().unwrap())
    }
    pub(super) fn device(&self, pointer: GuestAddress) -> Result<&Device32, H> {
        match &self.object(pointer)?.kind {
            ObjectKind32::Device(device) => Ok(device),
            _ => Err(H::INVALID_PARAM),
        }
    }
    pub(super) fn device_mut(&mut self, pointer: GuestAddress) -> Result<&mut Device32, H> {
        let slot = self.slot(pointer)?;
        match &mut self.records[slot].as_mut().unwrap().kind {
            ObjectKind32::Device(device) => Ok(device),
            _ => Err(H::INVALID_PARAM),
        }
    }
    fn policy_matching(
        &self,
        retain: impl Fn(&crate::input::dinput::device::DirectInputDevice32) -> bool,
    ) -> (bool, bool) {
        let mut policy = (false, false);
        for record in self.records.iter().flatten() {
            if let ObjectKind32::Device(device) = &record.kind
                && device.native.acquisition()
                    == crate::input::dinput::device::Acquisition32::Acquired
                && retain(&device.native)
                && let Some(level) = device.native.cooperative()
            {
                policy.0 |=
                    device.kind == crate::input::dinput::DeviceKind32::Mouse && level.exclusive;
                policy.1 |= device.kind == crate::input::dinput::DeviceKind32::Keyboard
                    && (level.exclusive || level.no_windows_key);
            }
        }
        policy
    }
    pub(super) fn input_policy(&self) -> (bool, bool) {
        self.policy_matching(|_| true)
    }
    pub(super) fn policy_after_sync(
        &self,
        source: &crate::input::source::InputSource32,
        active: u32,
    ) -> (bool, bool) {
        self.policy_matching(|native| !native.will_lose(source, active))
    }
    pub(in crate::process) fn epoch_changed(&mut self) {
        for record in self.records.iter_mut().flatten() {
            if let ObjectKind32::Device(device) = &mut record.kind {
                device.native.epoch_changed();
            }
        }
    }
}
impl From<crate::process::windows::objects::GuestComError32> for H {
    fn from(error: crate::process::windows::objects::GuestComError32) -> Self {
        use crate::process::windows::objects::GuestComError32 as E;
        match error {
            E::Allocation | E::Capacity => Self::OUT_OF_MEMORY,
            E::Com(crate::windows::com::ComError32::NoInterface) => Self::NO_INTERFACE,
            _ => Self::INVALID_PARAM,
        }
    }
}
impl From<crate::memory::MemoryError> for H {
    fn from(_: crate::memory::MemoryError) -> Self {
        Self::INVALID_PARAM
    }
}

impl DirectInputRuntime32 {
    pub(in crate::process) fn guest_reservations(&self) -> [crate::memory::PageRange; 2] {
        [self.config.objects, self.config.scratch]
    }
}
