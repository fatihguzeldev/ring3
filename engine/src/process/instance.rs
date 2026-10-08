#![forbid(unsafe_code)]

use std::pin::Pin;

use super::call::{CallError, PendingCall};
use super::callback::SuspendedCallback;
use super::installation::{RESIDENT_INSTALLATION_SLOTS, ResidentInstallation};

use crate::{
    abi::{
        arena::{self, ARENA_SIZE, HELPER_OFFSET, TRANSFER_OFFSET, TRANSFER_SIZE},
        memory_helper::{
            HELPER_SIZE, NarrowReadWidth, encode_byte_store_result, encode_helper_result,
            encode_narrow_helper_result, encode_word_store_result,
        },
    },
    cpu::dbt::{
        BlockSpec, CompileError, CompileLimits, CompiledRegion, GateSpec, RegistryError,
        ResidentRegistry, compile_embedded_entry_region, compile_embedded_region, emit_dispatcher,
    },
    memory::{
        AddressSpace, GuestAddress, MAX_WORD_WRITES32, MemoryError, PageRange, Permissions,
        WordWrite32,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostError {
    Closed,
    ProcessExited,
    InvalidRequest,
    InvalidArtifact,
    CodeInvalidated,
    GenerationExhausted,
    Memory(MemoryError),
    Loader(crate::loader::LoadError),
    Compile(CompileError),
    Resident(RegistryError),
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
    pub(super) dispatcher: Option<Vec<u8>>,
    pub(super) resident: Option<ResidentRegistry>,
    pub(super) resident_installations: [Option<ResidentInstallation>; RESIDENT_INSTALLATION_SLOTS],
    pub(super) key: u64,
    pub(super) generation: u32,
    pub(super) pending_call: Option<PendingCall>,
    pub(super) callback: Option<SuspendedCallback>,
    pub(super) call_token: u32,
    pub(super) image: Option<crate::loader::ImageMetadata32>,
    pub(super) image_input: Option<super::image_input::ImageInput>,
    pub(super) image_started: bool,
    pub(super) exit_code: Option<u32>,
    pub(super) windows_thread: crate::windows::ThreadState32,
    pub(super) virtual_allocations: Vec<PageRange>,
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
            dispatcher: Some(emit_dispatcher(key)),
            resident: None,
            resident_installations: [None; RESIDENT_INSTALLATION_SLOTS],
            key,
            generation: 0,
            pending_call: None,
            callback: None,
            call_token: 0,
            image: None,
            image_input: None,
            image_started: false,
            exit_code: None,
            windows_thread: crate::windows::ThreadState32::default(),
            virtual_allocations: Vec::new(),
        })
    }

    pub fn dispatcher_bytes(&self, key: u64) -> Result<&[u8], HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        self.dispatcher.as_deref().ok_or(HostError::Infrastructure)
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
        let memory = self.retained_memory()?;
        if self.exit_code.is_some() {
            return Err(HostError::ProcessExited);
        }
        Ok(memory)
    }

    fn retained_memory(&self) -> Result<&AddressSpace, HostError> {
        self.memory.as_ref().ok_or(HostError::Closed)
    }

    pub fn map(&mut self, address: u32, pages: u32, bits: u32) -> Result<(), HostError> {
        self.memory()?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let permissions = permissions(bits)?;
        let range = PageRange::new(GuestAddress(address), pages).map_err(HostError::Memory)?;
        memory
            .map_zeroed(range, permissions)
            .map_err(HostError::Memory)
    }

    pub fn protect(&mut self, address: u32, pages: u32, bits: u32) -> Result<(), HostError> {
        self.memory()?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let permissions = permissions(bits)?;
        let range = PageRange::new(GuestAddress(address), pages).map_err(HostError::Memory)?;
        memory
            .protect(range, permissions)
            .map_err(HostError::Memory)
    }

    pub fn unmap(&mut self, address: u32, pages: u32) -> Result<(), HostError> {
        self.memory()?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let range = PageRange::new(GuestAddress(address), pages).map_err(HostError::Memory)?;
        memory.unmap(range).map_err(HostError::Memory)?;
        self.revoke_virtual_allocations(range);
        Ok(())
    }

    pub fn upload(&mut self, address: u32, length: u32) -> Result<(), HostError> {
        self.memory()?;
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

    /// checks process authority; the wasm boundary also validates pointers and state.
    pub fn guard_dispatch_entry(&self, key: u64) -> Result<(), HostError> {
        self.memory()?;
        if key != self.key {
            return Err(HostError::InvalidArtifact);
        }
        if self.pending_call.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        if let Some(callback) = self.callback.as_ref() {
            let Some(record) = callback.authorized_resident_record() else {
                return Err(HostError::Call(CallError::Busy));
            };
            self.guard_resident_unit(key, record.callback_unit_id)?;
            self.guard_resident_unit(key, record.outer_unit_id)?;
            self.guard_resident_unit(key, callback.authorized_resident_active_id().unwrap())?;
        }
        Ok(())
    }

    pub fn guard(&self, key: u64, generation: u32) -> Result<(), HostError> {
        self.guard_artifact(key, generation)?;
        if self.pending_call.is_some()
            || self
                .callback
                .as_ref()
                .is_some_and(SuspendedCallback::is_resident)
        {
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

    pub fn read8(&mut self, address: u32) -> Result<(), HostError> {
        let memory = self.retained_memory()?;
        let mut bytes = [0; 1];
        let result = memory
            .read(GuestAddress(address), &mut bytes)
            .map(|()| u32::from(bytes[0]));
        self.write_narrow_helper(address, NarrowReadWidth::Byte, result)
    }

    pub fn read16(&mut self, address: u32) -> Result<(), HostError> {
        let memory = self.retained_memory()?;
        let mut bytes = [0; 2];
        let result = memory
            .read(GuestAddress(address), &mut bytes)
            .map(|()| u32::from(u16::from_le_bytes(bytes)));
        self.write_narrow_helper(address, NarrowReadWidth::Word, result)
    }

    pub fn read32(&mut self, address: u32) -> Result<(), HostError> {
        let memory = self.retained_memory()?;
        let mut bytes = [0; 4];
        let result = memory
            .read(GuestAddress(address), &mut bytes)
            .map(|()| u32::from_le_bytes(bytes));
        self.write_helper(result)
    }

    pub fn write8(&mut self, address: u32, value: u32) -> Result<(), HostError> {
        self.memory()?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let result = memory.write(GuestAddress(address), &[value as u8]);
        self.write_byte_store_helper(address, result)
    }

    pub fn write32(&mut self, address: u32, value: u32) -> Result<(), HostError> {
        self.memory()?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let result = memory
            .write(GuestAddress(address), &value.to_le_bytes())
            .map(|()| 0);
        self.write_helper(result)
    }

    pub fn write_words32(&mut self, count: u32) -> Result<(), HostError> {
        self.memory()?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        if count > MAX_WORD_WRITES32 as u32 {
            return Err(HostError::InvalidRequest);
        }
        let transfer =
            &self.arena.as_ref().get_ref()[TRANSFER_OFFSET..TRANSFER_OFFSET + TRANSFER_SIZE];
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

    pub fn store8(&mut self, address: u32, value: u32) -> Result<StoreCompletion, HostError> {
        self.guard(self.key, self.generation())?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let result = memory.write(GuestAddress(address), &[value as u8]);
        let succeeded = result.is_ok();
        self.write_byte_store_helper(address, result)?;
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

    pub fn store_resident8(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        value: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_resident(key, id)?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let result = memory.write(GuestAddress(address), &[value as u8]);
        let succeeded = result.is_ok();
        self.write_byte_store_helper(address, result)?;
        if succeeded {
            match self.resident_bytes(id) {
                Ok(_) => Ok(StoreCompletion::Complete),
                Err(HostError::Resident(RegistryError::CodeInvalidated)) => {
                    Ok(StoreCompletion::CodeInvalidated)
                }
                Err(error) => Err(error),
            }
        } else {
            Ok(StoreCompletion::Complete)
        }
    }

    pub fn store16(&mut self, address: u32, value: u32) -> Result<StoreCompletion, HostError> {
        self.guard(self.key, self.generation())?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let result = memory.write(GuestAddress(address), &(value as u16).to_le_bytes());
        let succeeded = result.is_ok();
        self.write_word_store_helper(address, result)?;
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

    pub fn store_resident16(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        value: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_resident(key, id)?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let result = memory.write(GuestAddress(address), &(value as u16).to_le_bytes());
        let succeeded = result.is_ok();
        self.write_word_store_helper(address, result)?;
        if succeeded {
            match self.resident_bytes(id) {
                Ok(_) => Ok(StoreCompletion::Complete),
                Err(HostError::Resident(RegistryError::CodeInvalidated)) => {
                    Ok(StoreCompletion::CodeInvalidated)
                }
                Err(error) => Err(error),
            }
        } else {
            Ok(StoreCompletion::Complete)
        }
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

    pub fn store_resident32(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        value: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_resident(key, id)?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let result = memory
            .write(GuestAddress(address), &value.to_le_bytes())
            .map(|()| 0);
        let succeeded = result.is_ok();
        self.write_helper(result)?;
        if succeeded {
            match self.resident_bytes(id) {
                Ok(_) => Ok(StoreCompletion::Complete),
                Err(HostError::Resident(RegistryError::CodeInvalidated)) => {
                    Ok(StoreCompletion::CodeInvalidated)
                }
                Err(error) => Err(error),
            }
        } else {
            Ok(StoreCompletion::Complete)
        }
    }

    pub fn close(&mut self) {
        self.image_input = None;
        self.pending_call = None;
        self.callback = None;
        self.artifact = None;
        self.dispatcher = None;
        self.resident = None;
        self.resident_installations = [None; RESIDENT_INSTALLATION_SLOTS];
        self.memory = None;
        self.virtual_allocations = Vec::new();
    }

    fn write_byte_store_helper(
        &mut self,
        address: u32,
        result: Result<(), MemoryError>,
    ) -> Result<(), HostError> {
        let output = &mut self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE];
        encode_byte_store_result(GuestAddress(address), result, output)
            .map_err(|_| HostError::Infrastructure)
    }

    fn write_word_store_helper(
        &mut self,
        address: u32,
        result: Result<(), MemoryError>,
    ) -> Result<(), HostError> {
        let output = &mut self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE];
        encode_word_store_result(GuestAddress(address), result, output)
            .map_err(|_| HostError::Infrastructure)
    }

    fn write_helper(&mut self, result: Result<u32, MemoryError>) -> Result<(), HostError> {
        let output = &mut self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE];
        encode_helper_result(result, output).map_err(|_| HostError::Infrastructure)
    }

    fn write_narrow_helper(
        &mut self,
        address: u32,
        width: NarrowReadWidth,
        result: Result<u32, MemoryError>,
    ) -> Result<(), HostError> {
        let output = &mut self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE];
        encode_narrow_helper_result(GuestAddress(address), width, result, output)
            .map_err(|_| HostError::Infrastructure)
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
    fn exhausted_versions_preserve_resident_store_ram_and_executing_snapshot() {
        let mut instance = EngineInstance::new(2, 7).unwrap();
        instance.map(0x1000, 1, 7).unwrap();
        instance.map(0x4000, 1, 7).unwrap();
        instance.write32(0x1000, 0x00eb_0389).unwrap();
        instance.write32(0x4000, 0x4433_2211).unwrap();
        instance.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
            .copy_from_slice(&[0, 0x10, 0, 0, 4, 0, 0, 0]);
        let id = instance.compile_resident(1).unwrap().get();
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
        let unit = instance.resident_bytes(id).unwrap().to_vec();
        let pointer = instance.resident_bytes(id).unwrap().as_ptr();
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

        for (address, value) in [(0x4000, u32::MAX), (0x1000, 0x00eb_0390)] {
            assert_eq!(
                instance.store_resident32(7, id, address, value),
                Ok(StoreCompletion::Complete)
            );
            assert_eq!(instance.arena(), expected_arena);
        }
        assert_eq!(instance.generation(), 0);
        assert_eq!(instance.artifact_bytes(), Err(HostError::InvalidArtifact));
        assert_eq!(instance.guard_resident(7, id), Ok(()));
        assert_eq!(instance.resident_bytes(id).unwrap(), unit);
        assert_eq!(instance.resident_bytes(id).unwrap().as_ptr(), pointer);
        let memory = instance.memory().unwrap();
        assert!(memory.is_code_current(&code_snapshot));
        assert!(memory.is_code_current(&data_snapshot));
        for (address, expected) in [
            (0x1000, [0x89, 0x03, 0xeb, 0]),
            (0x4000, [0x11, 0x22, 0x33, 0x44]),
        ] {
            let mut actual = [0; 4];
            memory.read(GuestAddress(address), &mut actual).unwrap();
            assert_eq!(actual, expected);
        }
    }

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

    fn byte_store_fixture() -> (EngineInstance, u64, u64) {
        let mut engine = EngineInstance::new(4, 0x1234_5678_9abc_def0).unwrap();
        for address in [0x1000, 0x3000, 0x5000, 0x8000] {
            engine.map(address, 1, 7).unwrap();
        }
        for (address, bytes) in [
            (0x1000, &[0x0f, 0x0b][..]),
            (0x3000, &[0x90, 0xeb, 0][..]),
            (0x5000, &[0x11, 0x22, 0x33, 0x44][..]),
            (0x8000, &[0, 0x30, 0, 0, 0xef, 0xcd, 0xab, 0x89][..]),
        ] {
            engine
                .memory
                .as_mut()
                .unwrap()
                .write(GuestAddress(address), bytes)
                .unwrap();
        }
        let transfer = &mut engine.arena_mut().unwrap()[140..164];
        for (index, word) in [0x1000_u32, 2, 0x3000, 3, 0x1000, 7]
            .into_iter()
            .enumerate()
        {
            transfer[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        engine.compile_with_gates(2, 1).unwrap();
        for (index, word) in [0x1000_u32, 2, 0x1000, 7].into_iter().enumerate() {
            engine.arena_mut().unwrap()[140 + index * 4..144 + index * 4]
                .copy_from_slice(&word.to_le_bytes());
        }
        let outer = engine.compile_resident_with_gates(1, 1).unwrap().get();
        engine.arena_mut().unwrap()[140..148].copy_from_slice(&[0, 0x30, 0, 0, 3, 0, 0, 0]);
        let home = engine.compile_resident(1).unwrap().get();
        (engine, outer, home)
    }

    fn byte_store_packet(version: u16, fields: [u32; 6]) -> [u8; 40] {
        let mut bytes = [0; 40];
        bytes[..16].copy_from_slice(&[82, 51, 77, 72, 0, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]);
        bytes[4..6].copy_from_slice(&version.to_le_bytes());
        for (index, field) in fields.into_iter().enumerate() {
            bytes[16 + index * 4..20 + index * 4].copy_from_slice(&field.to_le_bytes());
        }
        bytes
    }

    fn byte_store_helper_only(
        engine: &EngineInstance,
        before: &[u8],
        version: u16,
        fields: [u32; 6],
    ) {
        let mut expected = before.to_vec();
        expected[100..140].copy_from_slice(&byte_store_packet(version, fields));
        assert_eq!(engine.arena(), expected);
    }

    fn byte_store_ram(engine: &EngineInstance, address: u32, length: usize) -> Vec<u8> {
        let mut bytes = vec![0; length];
        engine
            .memory
            .as_ref()
            .unwrap()
            .read(GuestAddress(address), &mut bytes)
            .unwrap();
        bytes
    }

    fn byte_store_inject_gate(engine: &mut EngineInstance) {
        // canonical native stop model; actual call execution is proved by the wasm fixture.
        let state = crate::cpu::x86::State32 {
            registers: [1, 2, 3, 4, 0x8000, 6, 7, 8],
            eip: 0x1000,
            eflags: 0xcd7,
        };
        crate::abi::x86::encode_state(&state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
        crate::abi::x86::encode_exit_v3(
            &crate::cpu::ExecutionExit {
                retired: 3,
                reason: crate::cpu::ExitReason::Gate { id: 7 },
            },
            &mut engine.arena_mut().unwrap()[56..96],
        )
        .unwrap();
    }

    #[test]
    fn byte_store_version_exhaustion_preserves_ram_identity_snapshots_and_all_three_owners() {
        let (mut engine, outer, home) = byte_store_fixture();
        let generation = engine.generation();
        let artifact = engine.artifact_bytes().unwrap().to_vec();
        let artifact_pointer = engine.artifact_bytes().unwrap().as_ptr();
        let resident = engine.resident_bytes(home).unwrap().to_vec();
        let resident_pointer = engine.resident_bytes(home).unwrap().as_ptr();
        let identity = engine.memory().unwrap().identity();
        let snapshots = [0x1000, 0x3000, 0x5000].map(|address| {
            engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(address), 2)
                .unwrap()
        });
        engine.memory.as_mut().unwrap().exhaust_versions_for_test();
        for address in [0x5001, 0x1000, 0x3000] {
            for method in 0..3 {
                let before = engine.arena().to_vec();
                match method {
                    0 => engine.write8(address, u32::MAX).unwrap(),
                    1 => assert_eq!(
                        engine.store8(address, u32::MAX),
                        Ok(StoreCompletion::Complete)
                    ),
                    _ => assert_eq!(
                        engine.store_resident8(engine.key, home, address, u32::MAX),
                        Ok(StoreCompletion::Complete)
                    ),
                }
                byte_store_helper_only(&engine, &before, 3, [2, 0, 1, 0, 0, 1]);
            }
        }
        assert_eq!(byte_store_ram(&engine, 0x1000, 2), [0x0f, 0x0b]);
        assert_eq!(byte_store_ram(&engine, 0x3000, 3), [0x90, 0xeb, 0]);
        assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x11, 0x22, 0x33, 0x44]);
        assert_eq!(engine.memory().unwrap().identity(), identity);
        assert!(
            snapshots
                .iter()
                .all(|snapshot| engine.memory().unwrap().is_code_current(snapshot))
        );
        assert_eq!(engine.generation(), generation);
        assert_eq!(engine.artifact_bytes().unwrap(), artifact);
        assert_eq!(engine.artifact_bytes().unwrap().as_ptr(), artifact_pointer);
        assert_eq!(engine.resident_bytes(home).unwrap(), resident);
        assert_eq!(
            engine.resident_bytes(home).unwrap().as_ptr(),
            resident_pointer
        );
        engine.guard(engine.key, generation).unwrap();
        engine.guard_resident(engine.key, outer).unwrap();
        engine.guard_resident(engine.key, home).unwrap();
    }

    #[test]
    fn byte_store_pending_is_busy_before_effect_while_direct_and_old_store4_remain_usable() {
        for resident in [false, true] {
            let (mut engine, outer, home) = byte_store_fixture();
            byte_store_inject_gate(&mut engine);
            if resident {
                engine
                    .capture_resident_call(
                        engine.key,
                        outer,
                        crate::windows::CallingConvention32::Cdecl,
                        1,
                    )
                    .unwrap();
            } else {
                engine
                    .capture_call(
                        engine.key,
                        engine.generation(),
                        crate::windows::CallingConvention32::Cdecl,
                        1,
                    )
                    .unwrap();
            }
            let token = engine.pending_call.as_ref().unwrap().token;
            let before = engine.arena().to_vec();
            let ram = byte_store_ram(&engine, 0x5000, 4);
            for address in [0x5000, 0xffff_ffff] {
                assert_eq!(
                    engine.store8(address, 0xff),
                    Err(HostError::Call(CallError::Busy))
                );
                assert_eq!(
                    engine.store_resident8(engine.key, home, address, 0xff),
                    Err(HostError::Call(CallError::Busy))
                );
                assert_eq!(engine.arena(), before);
                assert_eq!(byte_store_ram(&engine, 0x5000, 4), ram);
            }
            assert_eq!(
                engine.store_resident8(engine.key ^ (1 << 32), home, 0x5000, 0xff),
                Err(HostError::InvalidArtifact)
            );
            assert_eq!(engine.arena(), before);
            engine.write8(0x5001, 0x180).unwrap();
            byte_store_helper_only(&engine, &before, 3, [0, 0, 0, 0, 0, 1]);
            let before_store4 = engine.arena().to_vec();
            assert_eq!(
                engine.store32(0x5000, 0x8877_6655),
                Ok(StoreCompletion::Complete)
            );
            byte_store_helper_only(&engine, &before_store4, 1, [0, 0, 0, 0, 0, 0]);
            assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x55, 0x66, 0x77, 0x88]);
            assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
            engine
                .memory
                .as_mut()
                .unwrap()
                .write(GuestAddress(0x1000), &[0x0f])
                .unwrap();
            let before_stale = engine.arena().to_vec();
            assert_eq!(engine.store8(0x5000, 0xff), Err(HostError::CodeInvalidated));
            assert_eq!(
                engine.store_resident8(engine.key, outer, 0x5000, 0xff),
                Err(HostError::Resident(RegistryError::CodeInvalidated))
            );
            assert_eq!(engine.arena(), before_stale);
        }
    }

    #[test]
    fn byte_store_resident_callback_uses_only_authorized_active_current_owner() {
        use crate::process::callback::{SuspendedCallback, SuspendedRecord};
        let (mut engine, outer_id, home_id) = byte_store_fixture();
        byte_store_inject_gate(&mut engine);
        engine
            .capture_resident_call(
                engine.key,
                outer_id,
                crate::windows::CallingConvention32::Cdecl,
                1,
            )
            .unwrap();
        let outer = engine.pending_call.take().unwrap();
        let token = outer.token + 1;
        // private native activity model; this is not callback execution or installation evidence.
        engine.callback = Some(SuspendedCallback {
            outer,
            record: SuspendedRecord::Resident {
                record: crate::abi::resident_callback::ResidentCallbackRecord32 {
                    token,
                    outer_token: token - 1,
                    phase: 1,
                    outcome: 0,
                    entry_pc: 0x3000,
                    entry_esp: 0x8000,
                    return_pc: 0x1000,
                    return_id: 8,
                    stack_words: 0,
                    result: 0,
                    outer_unit_id: outer_id,
                    callback_unit_id: home_id,
                },
                authorized: false,
                active_unit_id: home_id,
            },
        });
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.store8(0x5000, 0xff),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(
            engine.store_resident8(engine.key, home_id, 0x5000, 0xff),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(engine.arena(), before);
        if let SuspendedRecord::Resident { authorized, .. } =
            &mut engine.callback.as_mut().unwrap().record
        {
            *authorized = true;
        }
        assert_eq!(
            engine.store8(0x5000, 0xff),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(
            engine.store_resident8(engine.key, outer_id, 0x5000, 0xff),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(
            engine.store_resident8(engine.key, home_id, 0x5001, 0x180),
            Ok(StoreCompletion::Complete)
        );
        byte_store_helper_only(&engine, &before, 3, [0, 0, 0, 0, 0, 1]);
        assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x11, 0x80, 0x33, 0x44]);
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0x1000), &[0x0f])
            .unwrap();
        let before_stale = engine.arena().to_vec();
        assert_eq!(
            engine.store_resident8(engine.key, home_id, 0x5000, 0xff),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
        assert_eq!(engine.arena(), before_stale);

        let (mut engine, _, home) = byte_store_fixture();
        byte_store_inject_gate(&mut engine);
        engine
            .capture_call(
                engine.key,
                engine.generation(),
                crate::windows::CallingConvention32::Cdecl,
                1,
            )
            .unwrap();
        let outer = engine.pending_call.take().unwrap();
        let token = outer.token + 1;
        engine.callback = Some(SuspendedCallback {
            outer,
            record: SuspendedRecord::Replacement(crate::abi::callback::CallbackRecord32 {
                token,
                outer_token: token - 1,
                phase: 1,
                outcome: 0,
                entry_pc: 0x3000,
                entry_esp: 0x8000,
                return_pc: 0x1000,
                return_id: 8,
                stack_words: 0,
                result: 0,
                generation: engine.generation(),
            }),
        });
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.store_resident8(engine.key, home, 0x5000, 0xff),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.store8(0x5001, 0x180), Ok(StoreCompletion::Complete));
        byte_store_helper_only(&engine, &before, 3, [0, 0, 0, 0, 0, 1]);
    }

    #[test]
    fn byte_store_terminal_latch_and_closed_guard_preserve_retained_resources_before_close() {
        let (mut engine, outer, home) = byte_store_fixture();
        let identity = engine.memory().unwrap().identity();
        let snapshots = [0x1000, 0x3000, 0x5000].map(|address| {
            engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(address), 2)
                .unwrap()
        });
        let artifact = engine
            .artifact
            .as_ref()
            .unwrap()
            .wasm_bytes(engine.memory().unwrap())
            .unwrap()
            .to_vec();
        let resident = engine.resident_bytes(home).unwrap().to_vec();
        let generation = engine.generation();
        engine.image_started = true;
        engine.exit_code = Some(0xf123_4567);
        engine.arena.as_mut().get_mut()[..100].fill(0xa5);
        let before = engine.arena().to_vec();
        assert_eq!(engine.write8(0x5000, 0xff), Err(HostError::ProcessExited));
        assert_eq!(engine.store8(0x5000, 0xff), Err(HostError::ProcessExited));
        assert_eq!(
            engine.store_resident8(0, 0, 0x5000, 0xff),
            Err(HostError::ProcessExited)
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x11, 0x22, 0x33, 0x44]);
        let memory = engine.memory.as_ref().unwrap();
        assert_eq!(memory.identity(), identity);
        assert!(
            snapshots
                .iter()
                .all(|snapshot| memory.is_code_current(snapshot))
        );
        assert_eq!(
            engine
                .artifact
                .as_ref()
                .unwrap()
                .wasm_bytes(memory)
                .unwrap(),
            artifact
        );
        assert_eq!(
            engine
                .resident
                .as_ref()
                .unwrap()
                .get_raw(memory, home)
                .unwrap()
                .wasm_bytes(memory)
                .unwrap(),
            resident
        );
        assert!(
            engine
                .resident
                .as_ref()
                .unwrap()
                .get_raw(memory, outer)
                .is_ok()
        );
        assert_eq!(engine.generation(), generation);
        assert_eq!(engine.exit_code, Some(0xf123_4567));
        assert!(engine.image_started);
        engine.read8(0x5000).unwrap();
        byte_store_helper_only(&engine, &before, 2, [0, 0x11, 0, 0, 0, 1]);
        let before_close = engine.arena().to_vec();
        engine.close();
        assert_eq!(engine.write8(0x5000, 0xff), Err(HostError::Closed));
        assert_eq!(engine.store8(0x5000, 0xff), Err(HostError::Closed));
        assert_eq!(
            engine.store_resident8(0, 0, 0x5000, 0xff),
            Err(HostError::Closed)
        );
        assert_eq!(engine.arena(), before_close);
        assert!(engine.memory.is_none());
        assert!(engine.artifact.is_none());
        assert!(engine.resident.is_none());
    }

    #[test]
    fn word_store_cross_page_write_uses_one_content_version() {
        let (mut engine, outer, home) = byte_store_fixture();
        engine.unmap(0x8000, 1).unwrap();
        engine.map(0x6000, 1, 7).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0x5ffe), &[0x11, 0x22, 0x33, 0x44])
            .unwrap();
        for resident in [false, true] {
            let first = engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(0x5ffe), 4)
                .unwrap();
            let before = engine.arena().to_vec();
            assert_eq!(
                if resident {
                    engine.store_resident16(engine.key, home, 0x5fff, 0x9081)
                } else {
                    engine.store16(0x5fff, 0x9081)
                },
                Ok(StoreCompletion::Complete)
            );
            byte_store_helper_only(&engine, &before, 4, [0, 0, 0, 0, 0, 2]);
            assert_eq!(byte_store_ram(&engine, 0x5ffe, 4), [0x11, 0x81, 0x90, 0x44]);
            let after = engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(0x5ffe), 4)
                .unwrap();
            assert_eq!(after.versions[0].content, after.versions[1].content);
            assert!(after.versions[0].content > first.versions[0].content);
            assert!(after.versions[1].content > first.versions[1].content);
            assert!(!engine.memory().unwrap().is_code_current(&first));
            engine.guard(engine.key, engine.generation()).unwrap();
            engine.guard_resident(engine.key, outer).unwrap();
            engine.guard_resident(engine.key, home).unwrap();
        }
    }

    #[test]
    fn word_store_version_exhaustion_preserves_ram_versions_and_all_owners() {
        let (mut engine, outer, home) = byte_store_fixture();
        engine.unmap(0x8000, 1).unwrap();
        engine.map(0x6000, 1, 7).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0x5ffe), &[0x11, 0x22, 0x33, 0x44])
            .unwrap();
        let artifact = engine.artifact_bytes().unwrap().to_vec();
        let resident = engine.resident_bytes(home).unwrap().to_vec();
        let artifact_pointer = engine.artifact_bytes().unwrap().as_ptr();
        let resident_pointer = engine.resident_bytes(home).unwrap().as_ptr();
        let snapshots = [(0x1000, 2), (0x3000, 3), (0x5ffe, 4)].map(|(address, length)| {
            engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(address), length)
                .unwrap()
        });
        engine.memory.as_mut().unwrap().exhaust_versions_for_test();
        for address in [0x5fff, 0x1000, 0x3000] {
            for use_resident in [false, true] {
                let before = engine.arena().to_vec();
                assert_eq!(
                    if use_resident {
                        engine.store_resident16(engine.key, home, address, u32::MAX)
                    } else {
                        engine.store16(address, u32::MAX)
                    },
                    Ok(StoreCompletion::Complete)
                );
                byte_store_helper_only(&engine, &before, 4, [2, 0, 1, 0, 0, 2]);
            }
        }
        assert_eq!(byte_store_ram(&engine, 0x1000, 2), [0x0f, 0x0b]);
        assert_eq!(byte_store_ram(&engine, 0x3000, 3), [0x90, 0xeb, 0]);
        assert_eq!(byte_store_ram(&engine, 0x5ffe, 4), [0x11, 0x22, 0x33, 0x44]);
        assert!(
            snapshots
                .iter()
                .all(|snapshot| engine.memory().unwrap().is_code_current(snapshot))
        );
        assert_eq!(engine.artifact_bytes().unwrap(), artifact);
        assert_eq!(engine.resident_bytes(home).unwrap(), resident);
        assert_eq!(engine.artifact_bytes().unwrap().as_ptr(), artifact_pointer);
        assert_eq!(
            engine.resident_bytes(home).unwrap().as_ptr(),
            resident_pointer
        );
        engine.guard(engine.key, engine.generation()).unwrap();
        engine.guard_resident(engine.key, outer).unwrap();
        engine.guard_resident(engine.key, home).unwrap();
    }

    #[test]
    fn word_store_busy_terminal_and_closed_guards_leave_helper_and_ram_untouched() {
        for resident in [false, true] {
            let (mut engine, outer, home) = byte_store_fixture();
            byte_store_inject_gate(&mut engine);
            if resident {
                engine
                    .capture_resident_call(
                        engine.key,
                        outer,
                        crate::windows::CallingConvention32::Cdecl,
                        1,
                    )
                    .unwrap();
            } else {
                engine
                    .capture_call(
                        engine.key,
                        engine.generation(),
                        crate::windows::CallingConvention32::Cdecl,
                        1,
                    )
                    .unwrap();
            }
            let before = engine.arena().to_vec();
            for address in [0x5001, u32::MAX] {
                assert_eq!(
                    engine.store16(address, u32::MAX),
                    Err(HostError::Call(CallError::Busy))
                );
                assert_eq!(
                    engine.store_resident16(engine.key, home, address, u32::MAX),
                    Err(HostError::Call(CallError::Busy))
                );
                assert_eq!(engine.arena(), before);
                assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x11, 0x22, 0x33, 0x44]);
            }
            engine.exit_code = Some(7);
            assert_eq!(
                engine.store16(0x5001, u32::MAX),
                Err(HostError::ProcessExited)
            );
            assert_eq!(
                engine.store_resident16(0, 0, 0x5001, u32::MAX),
                Err(HostError::ProcessExited)
            );
            assert_eq!(engine.arena(), before);
            assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x11, 0x22, 0x33, 0x44]);
            engine.close();
            assert_eq!(engine.store16(0x5001, u32::MAX), Err(HostError::Closed));
            assert_eq!(
                engine.store_resident16(0, 0, 0x5001, u32::MAX),
                Err(HostError::Closed)
            );
            assert_eq!(engine.arena(), before);
        }
    }

    #[test]
    fn word_store_callback_requires_active_resident_and_current_outer_authority() {
        use crate::process::callback::{SuspendedCallback, SuspendedRecord};
        let (mut engine, outer_id, home_id) = byte_store_fixture();
        byte_store_inject_gate(&mut engine);
        engine
            .capture_resident_call(
                engine.key,
                outer_id,
                crate::windows::CallingConvention32::Cdecl,
                1,
            )
            .unwrap();
        let outer = engine.pending_call.take().unwrap();
        let token = outer.token + 1;
        engine.callback = Some(SuspendedCallback {
            outer,
            record: SuspendedRecord::Resident {
                record: crate::abi::resident_callback::ResidentCallbackRecord32 {
                    token,
                    outer_token: token - 1,
                    phase: 1,
                    outcome: 0,
                    entry_pc: 0x3000,
                    entry_esp: 0x8000,
                    return_pc: 0x1000,
                    return_id: 8,
                    stack_words: 0,
                    result: 0,
                    outer_unit_id: outer_id,
                    callback_unit_id: home_id,
                },
                authorized: false,
                active_unit_id: home_id,
            },
        });
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.store16(0x5001, 0x9081),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(
            engine.store_resident16(engine.key, home_id, 0x5001, 0x9081),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(engine.arena(), before);
        if let SuspendedRecord::Resident { authorized, .. } =
            &mut engine.callback.as_mut().unwrap().record
        {
            *authorized = true;
        }
        assert_eq!(
            engine.store_resident16(engine.key, outer_id, 0x5001, 0x9081),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(
            engine.store16(0x5001, 0x9081),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(
            engine.store_resident16(engine.key, home_id, 0x5001, 0x9081),
            Ok(StoreCompletion::Complete)
        );
        byte_store_helper_only(&engine, &before, 4, [0, 0, 0, 0, 0, 2]);
        assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x11, 0x81, 0x90, 0x44]);
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0x1000), &[0x0f])
            .unwrap();
        let before_stale = engine.arena().to_vec();
        assert_eq!(
            engine.store_resident16(engine.key, home_id, 0x5001, 0xffff),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
        assert_eq!(engine.arena(), before_stale);
        assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x11, 0x81, 0x90, 0x44]);
    }
}
