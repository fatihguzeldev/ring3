use super::{D3D8Config32, D3D8Error32, objects::Object32};
use crate::process::windows::objects::GuestComObjects32;
use crate::{
    cpu::dbt::GateSpec,
    graphics::d3d8::{
        api::{HResult, InterfaceKind, MethodGate},
        device::Device,
    },
    memory::GuestAddress,
};

pub(in crate::process) struct D3D8Runtime32 {
    pub(super) config: D3D8Config32,
    pub(super) com: GuestComObjects32,
    pub(super) records: Vec<Option<Object32>>,
    pub(super) methods: Vec<Vec<GuestAddress>>,
    pub(super) locks: super::locks::LockRegistry32,
    pub(super) private_callbacks: Vec<super::resources::PendingPrivateCall>,
    pub(super) private_sequence: u64,
    pub(super) garbage: Vec<super::garbage::Garbage32>,
    pub(super) retired_devices: Vec<super::garbage::RetiredDevice32>,
    pub(super) deferred_private_releases: Vec<GuestAddress>,
    pub(super) device_slots: Vec<usize>,
    pub(super) readback_input: Option<super::readback_input::ReadbackInput32>,
}

impl D3D8Runtime32 {
    pub(in crate::process) fn new(config: D3D8Config32) -> Result<Self, D3D8Error32> {
        config
            .adapter
            .validate()
            .map_err(|_| D3D8Error32::InvalidConfiguration)?;
        config
            .device
            .validate()
            .map_err(|_| D3D8Error32::InvalidConfiguration)?;
        if config.adapter.read_scanline {
            return Err(D3D8Error32::InvalidConfiguration);
        }
        if config.max_devices == 0
            || config.max_devices > 64
            || config.max_objects < config.max_devices + 4
            || config.max_objects > 65_536
            || config.max_locks == 0
            || config.max_locks > 4096
            || config.objects.first < config.locks.first + config.locks.count
                && config.locks.first < config.objects.first + config.objects.count
        {
            return Err(D3D8Error32::InvalidConfiguration);
        }
        let com = GuestComObjects32::new(
            config.objects,
            config.max_objects as usize,
            config.max_objects as usize,
            InterfaceKind::ALL.len() * config.max_devices as usize * 2,
        )
        .map_err(|_| D3D8Error32::InvalidConfiguration)?;
        let mut records = Vec::new();
        records
            .try_reserve_exact(config.max_objects as usize)
            .map_err(|_| D3D8Error32::Allocation)?;
        records.resize_with(config.max_objects as usize, || None);
        let locks = super::locks::LockRegistry32::new(config.locks, config.max_locks)
            .map_err(|_| D3D8Error32::InvalidConfiguration)?;
        let mut device_slots = Vec::new();
        device_slots
            .try_reserve_exact(config.max_devices as usize)
            .map_err(|_| D3D8Error32::Allocation)?;
        Ok(Self {
            config,
            com,
            records,
            methods: Vec::new(),
            locks,
            private_callbacks: Vec::new(),
            private_sequence: 0,
            readback_input: None,
            garbage: Vec::new(),
            retired_devices: Vec::new(),
            deferred_private_releases: Vec::new(),
            device_slots,
        })
    }

    pub(super) fn initialize_methods(&mut self, gates: &[GateSpec]) -> Result<(), HResult> {
        if self.methods.len() == InterfaceKind::ALL.len()
            && InterfaceKind::ALL.iter().all(|interface| {
                self.methods[*interface as usize - 1]
                    .iter()
                    .enumerate()
                    .all(|(slot, address)| {
                        gates.iter().any(|gate| {
                            gate.id
                                == MethodGate {
                                    interface: *interface,
                                    slot: slot as u8,
                                }
                                .id()
                                && gate.entry == *address
                        })
                    })
            })
        {
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
                let entry = gates
                    .iter()
                    .find(|gate| gate.id == id)
                    .ok_or(HResult::INVALID_CALL)?
                    .entry;
                table.push(entry);
            }
            methods.push(table);
        }
        self.methods = methods;
        Ok(())
    }

    pub(super) fn vacant_record(&self) -> Result<usize, HResult> {
        self.records
            .iter()
            .position(Option::is_none)
            .ok_or(HResult::OUT_OF_MEMORY)
    }

    pub(super) fn device(&self, pointer: GuestAddress) -> Result<&Device, HResult> {
        let object = self.object(pointer)?;
        match &object.kind {
            super::objects::ObjectKind32::Device(device) => Ok(&device.backend),
            _ => Err(HResult::INVALID_CALL),
        }
    }

    pub(super) fn device_mut(&mut self, pointer: GuestAddress) -> Result<&mut Device, HResult> {
        let slot = self.record_slot(pointer)?;
        match &mut self.records[slot]
            .as_mut()
            .ok_or(HResult::INVALID_CALL)?
            .kind
        {
            super::objects::ObjectKind32::Device(device) => Ok(&mut device.backend),
            _ => Err(HResult::INVALID_CALL),
        }
    }
}

impl D3D8Runtime32 {
    pub(in crate::process) fn guest_reservations(&self) -> [crate::memory::PageRange; 2] {
        [self.config.objects, self.config.locks]
    }
}
