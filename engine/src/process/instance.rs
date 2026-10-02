#![forbid(unsafe_code)]

use std::pin::Pin;

use super::call::{CallError, PendingCall};
use super::callback::SuspendedCallback;

use crate::{
    abi::{
        arena::{self, ARENA_SIZE, HELPER_OFFSET, TRANSFER_OFFSET, TRANSFER_SIZE},
        memory_helper::{HELPER_SIZE, encode_helper_result},
    },
    cpu::dbt::{
        BlockSpec, CompileError, CompileLimits, CompiledRegion, GateSpec,
        compile_embedded_entry_region, compile_embedded_region,
    },
    memory::{
        AddressSpace, GuestAddress, MAX_WORD_WRITES32, MemoryError, PageRange, Permissions,
        WordWrite32,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostError {
    Closed,
    InvalidRequest,
    InvalidArtifact,
    CodeInvalidated,
    GenerationExhausted,
    Memory(MemoryError),
    Compile(CompileError),
    Infrastructure,
    Call(CallError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreCompletion {
    Complete,
    CodeInvalidated,
}

#[derive(Clone, Copy)]
pub(super) enum DescriptorFormat {
    BlockSpecs,
    Entries,
}

pub struct EngineInstance {
    pub(super) memory: Option<AddressSpace>,
    pub(super) arena: Pin<Box<[u8]>>,
    pub(super) artifact: Option<CompiledRegion>,
    pub(super) key: u64,
    pub(super) generation: u32,
    pub(super) pending_call: Option<PendingCall>,
    pub(super) callback: Option<SuspendedCallback>,
    pub(super) call_token: u32,
}

impl EngineInstance {
    pub fn new(pages: u32, key: u64) -> Result<Self, HostError> {
        if pages == 0 || pages > 4096 || key == 0 {
            return Err(HostError::InvalidRequest);
        }
        let memory = AddressSpace::new(pages).map_err(HostError::Memory)?;
        let mut arena = Vec::new();
        arena
            .try_reserve_exact(ARENA_SIZE)
            .map_err(|_| HostError::Infrastructure)?;
        arena.resize(ARENA_SIZE, 0);
        arena::initialize(&mut arena).map_err(|_| HostError::Infrastructure)?;
        Ok(Self {
            memory: Some(memory),
            arena: Box::into_pin(arena.into_boxed_slice()),
            artifact: None,
            key,
            generation: 0,
            pending_call: None,
            callback: None,
            call_token: 0,
        })
    }

    pub fn is_open(&self) -> bool {
        self.memory.is_some()
    }

    pub fn key(&self) -> u64 {
        self.key
    }

    pub fn arena(&self) -> &[u8] {
        self.arena.as_ref().get_ref()
    }

    pub fn arena_mut(&mut self) -> Result<&mut [u8], HostError> {
        self.memory()?;
        Ok(self.arena.as_mut().get_mut())
    }

    pub fn arena_address(&self) -> usize {
        self.arena.as_ptr() as usize
    }

    pub fn memory(&self) -> Result<&AddressSpace, HostError> {
        self.memory.as_ref().ok_or(HostError::Closed)
    }

    pub fn map(&mut self, address: u32, pages: u32, bits: u32) -> Result<(), HostError> {
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let permissions = permissions(bits)?;
        let range = PageRange::new(GuestAddress(address), pages).map_err(HostError::Memory)?;
        memory
            .map_zeroed(range, permissions)
            .map_err(HostError::Memory)
    }

    pub fn protect(&mut self, address: u32, pages: u32, bits: u32) -> Result<(), HostError> {
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let permissions = permissions(bits)?;
        let range = PageRange::new(GuestAddress(address), pages).map_err(HostError::Memory)?;
        memory
            .protect(range, permissions)
            .map_err(HostError::Memory)
    }

    pub fn unmap(&mut self, address: u32, pages: u32) -> Result<(), HostError> {
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let range = PageRange::new(GuestAddress(address), pages).map_err(HostError::Memory)?;
        memory.unmap(range).map_err(HostError::Memory)
    }

    pub fn upload(&mut self, address: u32, length: u32) -> Result<(), HostError> {
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        if length as usize > TRANSFER_SIZE {
            return Err(HostError::InvalidRequest);
        }
        let input =
            &self.arena.as_ref().get_ref()[TRANSFER_OFFSET..TRANSFER_OFFSET + length as usize];
        memory
            .write(GuestAddress(address), input)
            .map_err(HostError::Memory)
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
        self.memory()?;
        if self.pending_call.is_some() || self.callback.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        Self::check_region_counts(count, gate_count)?;
        let generation = self
            .generation
            .checked_add(1)
            .ok_or(HostError::GenerationExhausted)?;
        let artifact = self.prepare_artifact(count, gate_count, generation, format)?;
        self.artifact = Some(artifact);
        self.generation = generation;
        Ok(generation)
    }

    pub(super) fn check_region_counts(count: u32, gate_count: u32) -> Result<(), HostError> {
        if !(1..=8).contains(&count) || gate_count > count {
            return Err(HostError::InvalidRequest);
        }
        Ok(())
    }

    pub(super) fn prepare_artifact(
        &self,
        count: u32,
        gate_count: u32,
        generation: u32,
        format: DescriptorFormat,
    ) -> Result<CompiledRegion, HostError> {
        Self::check_region_counts(count, gate_count)?;
        let memory = self.memory()?;
        let transfer = &self.arena()[TRANSFER_OFFSET..];
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
            ),
            DescriptorFormat::Entries => compile_embedded_entry_region(
                memory,
                &entries[..count as usize],
                CompileLimits::default(),
                self.key,
                generation,
                &gates[..gate_count as usize],
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

    pub fn guard(&self, key: u64, generation: u32) -> Result<(), HostError> {
        self.guard_artifact(key, generation)?;
        if self.pending_call.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        Ok(())
    }

    pub(super) fn guard_artifact(&self, key: u64, generation: u32) -> Result<(), HostError> {
        self.memory()?;
        if key != self.key || generation == 0 || generation != self.generation() {
            return Err(HostError::InvalidArtifact);
        }
        self.artifact_bytes().map(|_| ())
    }

    pub fn read32(&mut self, address: u32) -> Result<(), HostError> {
        let memory = self.memory()?;
        let mut bytes = [0; 4];
        let result = memory
            .read(GuestAddress(address), &mut bytes)
            .map(|()| u32::from_le_bytes(bytes));
        self.write_helper(result)
    }

    pub fn write32(&mut self, address: u32, value: u32) -> Result<(), HostError> {
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let result = memory
            .write(GuestAddress(address), &value.to_le_bytes())
            .map(|()| 0);
        self.write_helper(result)
    }

    pub fn write_words32(&mut self, count: u32) -> Result<(), HostError> {
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        if count > MAX_WORD_WRITES32 as u32 {
            return Err(HostError::InvalidRequest);
        }
        let transfer = &self.arena.as_ref().get_ref()[TRANSFER_OFFSET..];
        let mut words = [WordWrite32 {
            address: GuestAddress(0),
            value: 0,
        }; MAX_WORD_WRITES32];
        for (index, word) in words[..count as usize].iter_mut().enumerate() {
            let offset = index * 8;
            word.address = GuestAddress(u32::from_le_bytes(
                transfer[offset..offset + 4].try_into().unwrap(),
            ));
            word.value = u32::from_le_bytes(transfer[offset + 4..offset + 8].try_into().unwrap());
        }
        memory
            .write_words32(&words[..count as usize])
            .map_err(HostError::Memory)
    }

    pub fn store32(&mut self, address: u32, value: u32) -> Result<StoreCompletion, HostError> {
        self.artifact_bytes()?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let result = memory
            .write(GuestAddress(address), &value.to_le_bytes())
            .map(|()| 0);
        let succeeded = result.is_ok();
        self.write_helper(result)?;
        if succeeded {
            match self.artifact_bytes() {
                Ok(_) => Ok(StoreCompletion::Complete),
                Err(HostError::CodeInvalidated) => Ok(StoreCompletion::CodeInvalidated),
                Err(error) => Err(error),
            }
        } else {
            Ok(StoreCompletion::Complete)
        }
    }

    pub fn close(&mut self) {
        self.pending_call = None;
        self.callback = None;
        self.artifact = None;
        self.memory = None;
    }

    fn write_helper(&mut self, result: Result<u32, MemoryError>) -> Result<(), HostError> {
        let output = &mut self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE];
        encode_helper_result(result, output).map_err(|_| HostError::Infrastructure)
    }
}

fn permissions(bits: u32) -> Result<Permissions, HostError> {
    u8::try_from(bits)
        .ok()
        .and_then(Permissions::from_bits)
        .ok_or(HostError::InvalidRequest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhausted_memory_versions_complete_store_without_committing_or_invalidating() {
        let mut instance = EngineInstance::new(2, 7).unwrap();
        instance.map(0x1000, 1, 7).unwrap();
        instance.map(0x4000, 1, 7).unwrap();
        instance.write32(0x1000, 0x9090_9090).unwrap();
        instance.write32(0x4000, 0x4433_2211).unwrap();
        instance.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
            .copy_from_slice(&[0, 0x10, 0, 0, 1, 0, 0, 0]);
        let generation = instance.compile(1).unwrap();
        let code_snapshot = instance
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(0x1000), 4)
            .unwrap();
        let data_snapshot = instance
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(0x4000), 4)
            .unwrap();
        let artifact = instance.artifact_bytes().unwrap().to_vec();
        let mut expected_arena = instance.arena().to_vec();
        expected_arena[100..140].copy_from_slice(&[
            0x52, 0x33, 0x4d, 0x48, 1, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 1,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]);
        instance
            .memory
            .as_mut()
            .unwrap()
            .exhaust_versions_for_test();

        assert_eq!(
            instance.store32(0x4000, u32::MAX),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!(instance.arena(), expected_arena);
        assert_eq!(instance.generation(), generation);
        assert_eq!(instance.artifact_bytes().unwrap(), artifact);
        assert_eq!(instance.guard(7, generation), Ok(()));
        let memory = instance.memory().unwrap();
        assert!(memory.is_code_current(&code_snapshot));
        assert!(memory.is_code_current(&data_snapshot));
        for (address, expected) in [(0x1000, [0x90; 4]), (0x4000, [0x11, 0x22, 0x33, 0x44])] {
            let mut bytes = [0; 4];
            memory.read(GuestAddress(address), &mut bytes).unwrap();
            assert_eq!(bytes, expected);
        }
    }

    #[test]
    fn last_generation_installs_once_and_never_wraps_or_replaces() {
        let mut instance = EngineInstance::new(1, 1).unwrap();
        instance.map(0x1000, 1, 7).unwrap();
        instance.arena_mut().unwrap()[TRANSFER_OFFSET] = 0x90;
        instance.upload(0x1000, 1).unwrap();
        let descriptor = &mut instance.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
        descriptor[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
        descriptor[4..].copy_from_slice(&1_u32.to_le_bytes());
        instance.generation = u32::MAX - 1;
        assert_eq!(instance.compile(1), Ok(u32::MAX));
        let bytes = instance.artifact_bytes().unwrap().to_vec();
        assert_eq!(instance.compile(1), Err(HostError::GenerationExhausted));
        assert_eq!(instance.generation(), u32::MAX);
        assert_eq!(instance.artifact_bytes().unwrap(), bytes);
        assert_eq!(instance.guard(1, u32::MAX), Ok(()));
        assert_eq!(instance.guard(1, 0), Err(HostError::InvalidArtifact));
    }
}
