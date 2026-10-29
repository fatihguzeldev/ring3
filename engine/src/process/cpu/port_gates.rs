use crate::{
    abi::{
        arena::STATE_OFFSET,
        x86::{STATE_SIZE, decode_state},
    },
    cpu::dbt::GateSpec,
    memory::{ExternalCodePatch, ExternalCodeWrite, GuestAddress},
    process::{CallError, EngineInstance, HostError},
    windows::CallingConvention32,
};

const UD2: [u8; 2] = [0x0f, 0x0b];
const PORT: [u8; 2] = [0xe6, 0xe1];

#[derive(Clone, Copy)]
struct OwnedGate {
    gate: GateSpec,
    patch: ExternalCodePatch,
    cleanup: Option<u16>,
}
impl OwnedGate {
    fn bytes(self) -> [u8; 5] {
        match self.cleanup {
            Some(cleanup) => [0xe6, 0xe1, 0xc2, cleanup as u8, (cleanup >> 8) as u8],
            None => [0xe6, 0xe1, 0, 0, 0],
        }
    }
    fn matches(self, memory: &crate::memory::AddressSpace) -> bool {
        if self.cleanup.is_some() {
            memory.external_code_patch_matches(&self.patch, self.bytes())
        } else {
            memory.external_code_patch_matches(&self.patch, PORT)
        }
    }
}

#[derive(Default)]
pub(in crate::process) struct PortGates32 {
    mode: u32,
    gates: Vec<OwnedGate>,
}

impl EngineInstance {
    pub(in crate::process) fn forget_unloaded_external_port_gates(&mut self) {
        // successful unload has retired the registry and mappings before any address reuse.
        let mut gates = std::mem::take(&mut self.external_port_gates.gates);
        gates.retain(|entry| {
            entry.cleanup.is_some()
                || self.external_gate_is_owned(entry.gate)
                || self.memory.as_ref().is_none_or(|memory| {
                    memory.mapping_version_at(entry.patch.address) == Ok(entry.patch.mapping)
                })
        });
        self.external_port_gates.gates = gates;
    }

    fn external_gate_is_system(&self, gate: GateSpec) -> bool {
        self.loaded_windows_modules()
            .any(|module| module.is_system() && module.gates().contains(&gate))
    }
    fn external_gate_is_owned(&self, gate: GateSpec) -> bool {
        self.external_gate_is_system(gate)
            || (self.module_gate_at(gate.entry.0) == Some(gate)
                && !self
                    .loaded_windows_modules()
                    .any(|module| module.gates().iter().any(|entry| entry.entry == gate.entry)))
    }
    fn external_guest_ret_cleanup(&self, gate: GateSpec) -> Option<u16> {
        if !matches!(
            gate.id,
            0x10001 | 0x40225 | 0x4022a | 0x4022e | 0x40232 | 0x4023f | 0x4023d | 0x40253 | 0x40255
        ) || !self.external_gate_is_system(gate)
            || gate.entry.0 & 15 != 0
        {
            return None;
        }
        let info = crate::process::windows_gate_info(gate.id)?;
        if info.convention != CallingConvention32::Stdcall {
            return None;
        }
        u16::try_from(info.stack_words.checked_mul(4)?).ok()
    }
    fn external_guest_ret_slot(
        &self,
        gate: GateSpec,
        prefix: [u8; 2],
    ) -> Result<ExternalCodePatch, HostError> {
        let mut expected = [0; 16];
        expected[..2].copy_from_slice(&prefix);
        let memory = self.memory()?;
        let mut actual = [0; 16];
        memory
            .fetch(gate.entry, &mut actual)
            .map_err(HostError::Memory)?;
        if actual != expected {
            return Err(HostError::InvalidRequest);
        }
        memory
            .external_code_patch(gate.entry, [prefix[0], prefix[1], 0, 0, 0])
            .map_err(HostError::Memory)
    }
    fn validate_external_port_gates(&self) -> Result<(), HostError> {
        let memory = self.memory()?;
        if self.external_port_gates.gates.iter().any(|entry| {
            !self.external_gate_is_owned(entry.gate)
                || (entry.cleanup.is_some()
                    && self.external_guest_ret_cleanup(entry.gate) != entry.cleanup)
                || !entry.matches(memory)
        }) {
            return Err(HostError::InvalidRequest);
        }
        Ok(())
    }
    fn external_guest_ret_pending(&self) -> bool {
        if self.external_port_gates.mode != 2 {
            return false;
        }
        if self.callback.is_some()
            || self.cpu_event.is_some()
            || self.exceptions.has_deliveries()
            || self.has_active_module_operation()
        {
            return true;
        }
        let Ok(state) = decode_state(&self.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]) else {
            return true;
        };
        self.external_port_gates
            .gates
            .iter()
            .filter(|entry| entry.cleanup.is_some())
            .any(|entry| {
                let start = entry.gate.entry.0 + 2;
                let end = entry.gate.entry.0 + 5;
                (start <= state.eip && state.eip < end) || self.threads.resumes_in(start, end)
            })
    }
    pub(in crate::process) fn external_cpu_gate_mode(&self) -> Result<[u32; 4], HostError> {
        self.validate_external_port_gates()?;
        Ok([
            self.external_port_gates.mode,
            self.external_port_gates.gates.len() as u32,
            self.external_port_gates
                .gates
                .iter()
                .filter(|entry| entry.cleanup.is_some())
                .count() as u32,
            u32::from(self.external_guest_ret_pending()),
        ])
    }
    pub(in crate::process) fn set_external_cpu_gate_mode(
        &mut self,
        mode: u32,
    ) -> Result<(u32, u32), HostError> {
        if mode > 2 || !self.external_cpu_memory_pinned {
            return Err(HostError::InvalidRequest);
        }
        let previous = self.external_port_gates.mode;
        if previous == 2 && mode != 2 && self.external_guest_ret_pending() {
            return Err(HostError::Call(CallError::Busy));
        }
        self.validate_external_port_gates()?;
        if previous == mode {
            return Ok((mode, self.external_port_gates.gates.len() as u32));
        }
        let mut target = Vec::new();
        let mut writes = Vec::new();
        if previous == 0 {
            let mut gates = self.module_gates()?;
            gates.retain(|gate| self.external_gate_is_owned(*gate));
            gates.sort_unstable_by_key(|gate| gate.entry.0);
            target
                .try_reserve_exact(gates.len())
                .map_err(|_| HostError::Infrastructure)?;
            writes
                .try_reserve_exact(gates.len())
                .map_err(|_| HostError::Infrastructure)?;
            for gate in gates {
                let cleanup = if mode == 2 {
                    self.external_guest_ret_cleanup(gate)
                } else {
                    None
                };
                let patch = if cleanup.is_some() {
                    self.external_guest_ret_slot(gate, UD2)?
                } else {
                    self.memory()?
                        .external_code_patch(gate.entry, UD2)
                        .map_err(HostError::Memory)?
                };
                let entry = OwnedGate {
                    gate,
                    patch,
                    cleanup,
                };
                writes.push(ExternalCodeWrite {
                    patch,
                    before: [0x0f, 0x0b, 0, 0, 0],
                    after: entry.bytes(),
                    length: if cleanup.is_some() { 5 } else { 2 },
                });
                target.push(entry);
            }
        } else {
            target
                .try_reserve_exact(self.external_port_gates.gates.len())
                .map_err(|_| HostError::Infrastructure)?;
            writes
                .try_reserve_exact(self.external_port_gates.gates.len())
                .map_err(|_| HostError::Infrastructure)?;
            for &old in &self.external_port_gates.gates {
                let cleanup = if mode == 2 {
                    self.external_guest_ret_cleanup(old.gate)
                } else {
                    None
                };
                if mode == 0 {
                    writes.push(ExternalCodeWrite {
                        patch: old.patch,
                        before: old.bytes(),
                        after: [0x0f, 0x0b, 0, 0, 0],
                        length: if old.cleanup.is_some() { 5 } else { 2 },
                    });
                    continue;
                }
                let entry = OwnedGate { cleanup, ..old };
                if cleanup != old.cleanup {
                    if cleanup.is_some() {
                        self.external_guest_ret_slot(old.gate, PORT)?;
                    }
                    writes.push(ExternalCodeWrite {
                        patch: old.patch,
                        before: old.bytes(),
                        after: entry.bytes(),
                        length: 5,
                    });
                }
                target.push(entry);
            }
        }
        let version = self
            .memory
            .as_mut()
            .unwrap()
            .external_patch_code_writes(&writes)
            .map_err(HostError::Memory)?;
        for entry in &mut target {
            if writes
                .iter()
                .any(|write| write.patch.slot == entry.patch.slot)
            {
                entry.patch.content = version;
            }
        }
        let count = target.len() as u32;
        self.external_port_gates = PortGates32 {
            mode,
            gates: target,
        };
        Ok((mode, count))
    }
    pub(in crate::process) fn external_cpu_gate_match(&self, pc: u32) -> Option<(u32, bool)> {
        let gate = crate::process::execution::cache::cache_gate_at(
            &self.modules,
            self.callback.as_ref(),
            pc,
        )?;
        let mut bytes = [0; 2];
        self.memory
            .as_ref()?
            .fetch(GuestAddress(pc), &mut bytes)
            .ok()?;
        if bytes == UD2 {
            return Some((gate.id, false));
        }
        if bytes != PORT || !self.external_cpu_memory_pinned || self.external_port_gates.mode == 0 {
            return None;
        }
        let index = self
            .external_port_gates
            .gates
            .binary_search_by_key(&pc, |entry| entry.gate.entry.0)
            .ok()?;
        let entry = self.external_port_gates.gates[index];
        (entry.gate == gate && entry.matches(self.memory.as_ref()?)).then_some((gate.id, true))
    }
    pub(in crate::process) fn external_cpu_lookup_gate(
        &mut self,
        pc: u32,
    ) -> Result<Option<(u32, bool)>, HostError> {
        let matched = self.external_cpu_gate_match(pc);
        if let Some((id, false)) = matched {
            let gate = GateSpec {
                entry: GuestAddress(pc),
                id,
            };
            if self.external_port_gates.mode != 0 && self.external_gate_is_owned(gate) {
                if self
                    .external_port_gates
                    .gates
                    .binary_search_by_key(&pc, |entry| entry.gate.entry.0)
                    .is_ok()
                {
                    return Err(HostError::InvalidRequest);
                }
                let cleanup = if self.external_port_gates.mode == 2 {
                    self.external_guest_ret_cleanup(gate)
                } else {
                    None
                };
                let patch = if cleanup.is_some() {
                    self.external_guest_ret_slot(gate, UD2)
                } else {
                    self.memory()?
                        .external_code_patch(gate.entry, UD2)
                        .map_err(HostError::Memory)
                };
                if let Ok(mut patch) = patch {
                    if self.external_port_gates.gates.iter().any(|entry| {
                        entry.patch.slot == patch.slot
                            && !entry.matches(self.memory.as_ref().unwrap())
                    }) {
                        return Err(HostError::InvalidRequest);
                    }
                    self.external_port_gates
                        .gates
                        .try_reserve(1)
                        .map_err(|_| HostError::Infrastructure)?;
                    let entry = OwnedGate {
                        gate,
                        patch,
                        cleanup,
                    };
                    let write = ExternalCodeWrite {
                        patch,
                        before: [0x0f, 0x0b, 0, 0, 0],
                        after: entry.bytes(),
                        length: if cleanup.is_some() { 5 } else { 2 },
                    };
                    let version = self
                        .memory
                        .as_mut()
                        .unwrap()
                        .external_patch_code_writes(&[write])
                        .map_err(HostError::Memory)?;
                    for existing in &mut self.external_port_gates.gates {
                        if existing.patch.slot == patch.slot {
                            existing.patch.content = version;
                        }
                    }
                    patch.content = version;
                    let index = self
                        .external_port_gates
                        .gates
                        .partition_point(|entry| entry.gate.entry.0 < pc);
                    self.external_port_gates
                        .gates
                        .insert(index, OwnedGate { patch, ..entry });
                }
            }
        }
        Ok(matched)
    }
}
