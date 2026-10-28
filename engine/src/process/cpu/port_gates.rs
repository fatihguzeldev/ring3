use crate::{
    cpu::dbt::GateSpec,
    memory::{ExternalCodePatch, GuestAddress},
    process::{EngineInstance, HostError},
};

const UD2: [u8; 2] = [0x0f, 0x0b];
const PORT: [u8; 2] = [0xe6, 0xe1];

struct OwnedGate {
    gate: GateSpec,
    patch: ExternalCodePatch,
}

#[derive(Default)]
pub(in crate::process) struct PortGates32 {
    enabled: bool,
    gates: Vec<OwnedGate>,
}

impl EngineInstance {
    fn external_gate_is_owned(&self, gate: GateSpec) -> bool {
        if self
            .loaded_windows_modules()
            .any(|module| module.is_system() && module.gates().iter().any(|entry| *entry == gate))
        {
            return true;
        }
        // the only module gate outside loaded image gates is the engine initializer return.
        self.module_gate_at(gate.entry.0) == Some(gate)
            && !self
                .loaded_windows_modules()
                .any(|module| module.gates().iter().any(|entry| entry.entry == gate.entry))
    }

    fn validate_external_port_gates(&self) -> Result<(), HostError> {
        let memory = self.memory()?;
        if self.external_port_gates.gates.iter().any(|entry| {
            !self.external_gate_is_owned(entry.gate)
                || !memory.external_code_patch_matches(&entry.patch, PORT)
        }) {
            return Err(HostError::InvalidRequest);
        }
        Ok(())
    }

    pub(in crate::process) fn set_external_cpu_gate_mode(
        &mut self,
        mode: u32,
    ) -> Result<(u32, u32), HostError> {
        if mode > 1 || !self.external_cpu_memory_pinned {
            return Err(HostError::InvalidRequest);
        }
        if self.external_port_gates.enabled {
            self.validate_external_port_gates()?;
            if mode == 1 {
                return Ok((1, self.external_port_gates.gates.len() as u32));
            }
            let mut patches = Vec::new();
            patches
                .try_reserve_exact(self.external_port_gates.gates.len())
                .map_err(|_| HostError::Infrastructure)?;
            patches.extend(
                self.external_port_gates
                    .gates
                    .iter()
                    .map(|entry| entry.patch),
            );
            self.memory
                .as_mut()
                .unwrap()
                .external_patch_code(&patches, PORT, UD2)
                .map_err(HostError::Memory)?;
            self.external_port_gates = PortGates32::default();
            return Ok((0, 0));
        }
        if mode == 0 {
            return Ok((0, 0));
        }
        let mut gates = self.module_gates()?;
        gates.retain(|gate| self.external_gate_is_owned(*gate));
        gates.sort_unstable_by_key(|gate| gate.entry.0);
        let mut owned = Vec::new();
        let mut patches = Vec::new();
        owned
            .try_reserve_exact(gates.len())
            .map_err(|_| HostError::Infrastructure)?;
        patches
            .try_reserve_exact(gates.len())
            .map_err(|_| HostError::Infrastructure)?;
        for gate in gates {
            let patch = self
                .memory()?
                .external_code_patch(gate.entry, UD2)
                .map_err(HostError::Memory)?;
            owned.push(OwnedGate { gate, patch });
            patches.push(patch);
        }
        let version = self
            .memory
            .as_mut()
            .unwrap()
            .external_patch_code(&patches, UD2, PORT)
            .map_err(HostError::Memory)?;
        for entry in &mut owned {
            entry.patch.content = version;
        }
        let count = owned.len() as u32;
        self.external_port_gates = PortGates32 {
            enabled: true,
            gates: owned,
        };
        Ok((1, count))
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
        if bytes != PORT || !self.external_cpu_memory_pinned || !self.external_port_gates.enabled {
            return None;
        }
        let index = self
            .external_port_gates
            .gates
            .binary_search_by_key(&pc, |entry| entry.gate.entry.0)
            .ok()?;
        let entry = &self.external_port_gates.gates[index];
        (entry.gate == gate
            && self
                .memory
                .as_ref()?
                .external_code_patch_matches(&entry.patch, PORT))
        .then_some((gate.id, true))
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
            if self.external_port_gates.enabled && self.external_gate_is_owned(gate) {
                if self
                    .external_port_gates
                    .gates
                    .binary_search_by_key(&pc, |entry| entry.gate.entry.0)
                    .is_ok()
                {
                    return Err(HostError::InvalidRequest);
                }
                if let Ok(mut patch) = self.memory()?.external_code_patch(gate.entry, UD2) {
                    if self.external_port_gates.gates.iter().any(|entry| {
                        entry.patch.slot == patch.slot
                            && !self
                                .memory
                                .as_ref()
                                .unwrap()
                                .external_code_patch_matches(&entry.patch, PORT)
                    }) {
                        return Err(HostError::InvalidRequest);
                    }
                    self.external_port_gates
                        .gates
                        .try_reserve(1)
                        .map_err(|_| HostError::Infrastructure)?;
                    let version = self
                        .memory
                        .as_mut()
                        .unwrap()
                        .external_patch_code(&[patch], UD2, PORT)
                        .map_err(HostError::Memory)?;
                    for entry in &mut self.external_port_gates.gates {
                        if entry.patch.slot == patch.slot {
                            entry.patch.content = version;
                        }
                    }
                    patch.content = version;
                    let index = self
                        .external_port_gates
                        .gates
                        .partition_point(|entry| entry.gate.entry.0 < pc);
                    self.external_port_gates
                        .gates
                        .insert(index, OwnedGate { gate, patch });
                }
            }
        }
        Ok(matched)
    }
}
