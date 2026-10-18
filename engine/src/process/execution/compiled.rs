use crate::process::{CallError, EngineInstance, HostError};
use crate::{
    abi::arena::{TRANSFER_OFFSET, TRANSFER_SIZE},
    cpu::dbt::{
        BlockSpec, CompileLimits, CompiledRegion, GateSpec, compile_embedded_entry_region,
        compile_embedded_region,
    },
    memory::GuestAddress,
};

#[derive(Clone, Copy)]
pub(in crate::process) enum DescriptorFormat {
    BlockSpecs,
    Entries,
}

impl EngineInstance {
    pub fn dispatcher_bytes(&self, key: u64) -> Result<&[u8], HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        self.dispatcher.as_deref().ok_or(HostError::Infrastructure)
    }

    pub fn compile(&mut self, count: u32) -> Result<u32, HostError> {
        self.compile_with_gates(count, 0)
    }

    pub fn compile_with_gates(&mut self, count: u32, gate_count: u32) -> Result<u32, HostError> {
        self.compile_descriptors(count, gate_count, DescriptorFormat::BlockSpecs)
    }

    pub fn compile_entries(&mut self, count: u32, gate_count: u32) -> Result<u32, HostError> {
        self.compile_descriptors(count, gate_count, DescriptorFormat::Entries)
    }

    fn compile_descriptors(
        &mut self,
        count: u32,
        gate_count: u32,
        format: DescriptorFormat,
    ) -> Result<u32, HostError> {
        #[cfg(feature = "observability")]
        let _capture = self.observer.enter();
        self.memory()?;
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        self.guard_detection_material_change()?;
        Self::check_region_counts(count, gate_count)?;
        let generation = self.next_artifact_generation()?;
        let artifact = self.prepare_artifact(count, gate_count, generation, format)?;
        self.artifact = Some(artifact);
        self.generation = generation;
        self.generation_issuer = generation;
        Ok(generation)
    }

    pub(in crate::process) fn next_artifact_generation(&self) -> Result<u32, HostError> {
        self.generation
            .max(self.generation_issuer)
            .checked_add(1)
            .ok_or(HostError::GenerationExhausted)
    }

    pub(in crate::process) fn check_region_counts(
        count: u32,
        gate_count: u32,
    ) -> Result<(), HostError> {
        if !(1..=8).contains(&count) || gate_count > count {
            return Err(HostError::InvalidRequest);
        }
        Ok(())
    }

    pub(in crate::process) fn prepare_artifact(
        &self,
        count: u32,
        gate_count: u32,
        generation: u32,
        format: DescriptorFormat,
    ) -> Result<CompiledRegion, HostError> {
        Self::check_region_counts(count, gate_count)?;
        let memory = self.memory()?;
        let cs = self.code_fetch_context()?;
        let transfer = &self.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE];
        let stride = match format {
            DescriptorFormat::BlockSpecs => 8,
            DescriptorFormat::Entries => 4,
        };
        let mut entries = [GuestAddress(0); 8];
        let mut specs = [BlockSpec {
            entry: GuestAddress(0),
            byte_length: 0,
        }; 8];
        for index in 0..count as usize {
            let offset = index * stride;
            let entry = GuestAddress(u32::from_le_bytes(
                transfer[offset..offset + 4].try_into().unwrap(),
            ));
            match format {
                DescriptorFormat::BlockSpecs => {
                    specs[index] = BlockSpec {
                        entry,
                        byte_length: u32::from_le_bytes(
                            transfer[offset + 4..offset + 8].try_into().unwrap(),
                        ),
                    };
                }
                DescriptorFormat::Entries => entries[index] = entry,
            }
        }
        let mut gates = [GateSpec {
            entry: GuestAddress(0),
            id: 0,
        }; 8];
        for (index, gate) in gates[..gate_count as usize].iter_mut().enumerate() {
            let offset = count as usize * stride + index * 8;
            gate.entry = GuestAddress(u32::from_le_bytes(
                transfer[offset..offset + 4].try_into().unwrap(),
            ));
            gate.id = u32::from_le_bytes(transfer[offset + 4..offset + 8].try_into().unwrap());
        }
        match format {
            DescriptorFormat::BlockSpecs => compile_embedded_region(
                memory,
                &specs[..count as usize],
                CompileLimits::default(),
                self.key,
                generation,
                &gates[..gate_count as usize],
                cs,
                crate::cpu::x86::decode::DecodeAuthority::new(self.system_register_bank),
            ),
            DescriptorFormat::Entries => compile_embedded_entry_region(
                memory,
                &entries[..count as usize],
                CompileLimits::default(),
                self.key,
                generation,
                &gates[..gate_count as usize],
                cs,
                crate::cpu::x86::decode::DecodeAuthority::new(self.system_register_bank),
            ),
        }
        .map_err(HostError::Compile)
    }

    pub fn generation(&self) -> u32 {
        if self.is_open() && self.artifact.is_some() {
            self.generation
        } else {
            0
        }
    }

    pub fn artifact_bytes(&self) -> Result<&[u8], HostError> {
        let memory = self.memory()?;
        self.artifact
            .as_ref()
            .ok_or(HostError::InvalidArtifact)?
            .wasm_bytes(memory)
            .map_err(|_| HostError::CodeInvalidated)
    }
}

#[cfg(test)]
#[path = "compiled/identity/tests.rs"]
mod identity;
