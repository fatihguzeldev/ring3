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
    #[cfg(feature = "external-cpu")]
    pub(in crate::process) fn external_cpu_scalar_gate_safe(
        &self,
        memory: &crate::memory::AddressSpace,
        pointer: GuestAddress,
    ) -> bool {
        self.external_cpu_device_gate_safe(memory, pointer, false)
    }

    #[cfg(feature = "external-cpu")]
    fn external_cpu_device_gate_safe(
        &self,
        memory: &crate::memory::AddressSpace,
        pointer: GuestAddress,
        sealed: bool,
    ) -> bool {
        if !self.deferred_private_releases.is_empty()
            || !self.private_callbacks.is_empty()
            || !self.garbage.is_empty()
            || !self.retired_devices.is_empty()
            || self.readback_input.is_some()
            || self.com.preflight(memory, pointer).is_err()
        {
            return false;
        }
        self.device(pointer).is_ok_and(|device| {
            if sealed {
                device.sealed_state_overlap_ready()
            } else {
                !device.queue().is_draining() && !device.queue().is_sealed()
            }
        })
    }

    #[cfg(feature = "external-cpu")]
    pub(in crate::process) fn external_cpu_binding_gate_safe(
        &self,
        memory: &crate::memory::AddressSpace,
        pointer: GuestAddress,
        slot: u8,
        args: &[u32],
    ) -> bool {
        self.external_cpu_binding_gate_safe_with_queue(memory, pointer, slot, args, false)
    }

    #[cfg(feature = "external-cpu")]
    pub(in crate::process) fn external_cpu_overlap_gate_safe(
        &self,
        memory: &crate::memory::AddressSpace,
        gate: MethodGate,
        args: &[u32],
    ) -> bool {
        use crate::memory::Access;
        if gate.interface != InterfaceKind::Device
            || !matches!(gate.slot, 37 | 40 | 41 | 42 | 46 | 50 | 61 | 63 | 83 | 85)
            || args.len() != usize::from(gate.method().stack_words)
        {
            return false;
        }
        let pointer = GuestAddress(args[0]);
        if matches!(gate.slot, 61 | 83 | 85) {
            return self
                .external_cpu_binding_gate_safe_with_queue(memory, pointer, gate.slot, args, true);
        }
        if !self.external_cpu_device_gate_safe(memory, pointer, true) {
            return false;
        }
        let input = match gate.slot {
            37 => Some((args[2], 64)),
            40 => Some((args[1], 24)),
            42 => Some((args[1], 68)),
            _ => None,
        };
        if input.is_some_and(|(address, length)| {
            address == 0
                || memory
                    .check_span(GuestAddress(address), length, Access::Read)
                    .is_err()
        }) {
            return false;
        }
        gate.slot != 41
            || (args[1] != 0
                && memory.external_write_is_stable(GuestAddress(args[1]), 24)
                && memory.ensure_transaction_versions(1).is_ok())
    }

    #[cfg(feature = "external-cpu")]
    fn external_cpu_binding_gate_safe_with_queue(
        &self,
        memory: &crate::memory::AddressSpace,
        pointer: GuestAddress,
        slot: u8,
        args: &[u32],
        sealed: bool,
    ) -> bool {
        use crate::graphics::d3d8::device::Binding;
        if !self.external_cpu_device_gate_safe(memory, pointer, sealed)
            || args.len()
                != match slot {
                    61 | 85 => 3,
                    83 => 4,
                    _ => return false,
                }
        {
            return false;
        }
        let preflight = || -> Result<(), HResult> {
            let (raw, expected) = match slot {
                61 => (args[2], InterfaceKind::BaseTexture),
                83 => (args[2], InterfaceKind::VertexBuffer),
                _ => (args[1], InterfaceKind::IndexBuffer),
            };
            let handle = if raw == 0 {
                0
            } else {
                let object = self.object(GuestAddress(raw))?;
                if !object.interface.supports(expected) {
                    return Err(HResult::INVALID_CALL);
                }
                self.resource(GuestAddress(raw), pointer)?;
                self.com.preflight(memory, GuestAddress(raw))?;
                let counter = self.counter_pointer(object.pointer)?;
                self.com.preflight(memory, counter)?;
                object.pointer.0
            };
            let binding = match slot {
                61 => Binding::Texture {
                    stage: args[1],
                    handle,
                },
                83 => Binding::Stream {
                    stream: args[1],
                    handle,
                    stride: args[3],
                },
                _ => Binding::Indices {
                    handle,
                    base_vertex: args[2],
                },
            };
            let device = self.device(pointer)?;
            let prepared = device.prepare_binding(binding).map_err(HResult::from)?;
            device
                .can_commit_binding(&prepared)
                .map_err(HResult::from)?;
            if let Some(released) = prepared.released_resource() {
                self.resource(GuestAddress(released), pointer)?;
                self.com.preflight(memory, GuestAddress(released))?;
                let counter = self.counter_pointer(GuestAddress(released))?;
                self.com.preflight(memory, counter)?;
                if self.com.references(counter)? == 0 {
                    return Err(HResult::INVALID_CALL);
                }
            }
            Ok(())
        };
        preflight().is_ok()
    }

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
