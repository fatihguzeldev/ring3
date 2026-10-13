mod cpu_write;

use crate::process::{EngineInstance, HostError, StoreCompletion};
use crate::{
    abi::{
        arena::{HELPER_OFFSET, TRANSFER_OFFSET, TRANSFER_SIZE},
        memory_helper::{
            HELPER_SIZE, NarrowReadWidth, encode_byte_exchange_result, encode_byte_store_result,
            encode_helper_result, encode_narrow_helper_result, encode_word_exchange_result,
            encode_word_store_result,
        },
    },
    memory::{GuestAddress, MAX_WORD_WRITES32, MemoryError, WordWrite32},
};

impl EngineInstance {
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
        self.guard_material(self.key, self.generation())?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory.write(GuestAddress(address), &[value as u8]);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_byte_store_helper(address, result)?;
        self.cpu_store_currentness(code_changed, None)
    }

    pub fn store_resident8(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        value: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory.write(GuestAddress(address), &[value as u8]);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_byte_store_helper(address, result)?;
        self.cpu_store_currentness(code_changed, Some(id))
    }

    pub fn store16(&mut self, address: u32, value: u32) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory.write(GuestAddress(address), &(value as u16).to_le_bytes());
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_word_store_helper(address, result)?;
        self.cpu_store_currentness(code_changed, None)
    }

    pub fn store_resident16(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        value: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory.write(GuestAddress(address), &(value as u16).to_le_bytes());
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_word_store_helper(address, result)?;
        self.cpu_store_currentness(code_changed, Some(id))
    }

    pub fn store32(&mut self, address: u32, value: u32) -> Result<StoreCompletion, HostError> {
        // the legacy word store keeps pending-call admission while checking interpretation.
        self.artifact_bytes()?;
        self.check_decode_key(self.artifact.as_ref().unwrap())?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory
            .write(GuestAddress(address), &value.to_le_bytes())
            .map(|()| 0);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_helper(result)?;
        self.cpu_store_currentness(code_changed, None)
    }

    pub fn store_resident32(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        value: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory
            .write(GuestAddress(address), &value.to_le_bytes())
            .map(|()| 0);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_helper(result)?;
        self.cpu_store_currentness(code_changed, Some(id))
    }

    pub fn exchange16(&mut self, address: u32, value: u32) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory
            .exchange16(GuestAddress(address), value as u16)
            .map(u32::from);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_word_exchange_helper(address, result)?;
        self.cpu_store_currentness(code_changed, None)
    }

    pub fn exchange_resident16(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        value: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory
            .exchange16(GuestAddress(address), value as u16)
            .map(u32::from);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_word_exchange_helper(address, result)?;
        self.cpu_store_currentness(code_changed, Some(id))
    }

    pub fn exchange8(&mut self, address: u32, value: u32) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory
            .exchange8(GuestAddress(address), value as u8)
            .map(u32::from);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_byte_exchange_helper(address, result)?;
        self.cpu_store_currentness(code_changed, None)
    }

    pub fn exchange_resident8(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        value: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory
            .exchange8(GuestAddress(address), value as u8)
            .map(u32::from);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_byte_exchange_helper(address, result)?;
        self.cpu_store_currentness(code_changed, Some(id))
    }

    pub fn exchange32(&mut self, address: u32, value: u32) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory.exchange32(GuestAddress(address), value);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_helper(result)?;
        self.cpu_store_currentness(code_changed, None)
    }

    pub fn exchange_resident32(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        value: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory.exchange32(GuestAddress(address), value);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_helper(result)?;
        self.cpu_store_currentness(code_changed, Some(id))
    }

    pub fn compare_exchange8(
        &mut self,
        address: u32,
        expected: u32,
        desired: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory
            .compare_exchange8(GuestAddress(address), expected as u8, desired as u8)
            .map(u32::from);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_byte_exchange_helper(address, result)?;
        self.cpu_store_currentness(code_changed, None)
    }

    pub fn compare_exchange_resident8(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        expected: u32,
        desired: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory
            .compare_exchange8(GuestAddress(address), expected as u8, desired as u8)
            .map(u32::from);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_byte_exchange_helper(address, result)?;
        self.cpu_store_currentness(code_changed, Some(id))
    }

    pub fn compare_exchange16(
        &mut self,
        address: u32,
        expected: u32,
        desired: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory
            .compare_exchange16(GuestAddress(address), expected as u16, desired as u16)
            .map(u32::from);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_word_exchange_helper(address, result)?;
        self.cpu_store_currentness(code_changed, None)
    }

    pub fn compare_exchange_resident16(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        expected: u32,
        desired: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory
            .compare_exchange16(GuestAddress(address), expected as u16, desired as u16)
            .map(u32::from);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_word_exchange_helper(address, result)?;
        self.cpu_store_currentness(code_changed, Some(id))
    }

    pub fn compare_exchange32(
        &mut self,
        address: u32,
        expected: u32,
        desired: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory.compare_exchange32(GuestAddress(address), expected, desired);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_helper(result)?;
        self.cpu_store_currentness(code_changed, None)
    }

    pub fn compare_exchange_resident32(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        expected: u32,
        desired: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory.compare_exchange32(GuestAddress(address), expected, desired);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_helper(result)?;
        self.cpu_store_currentness(code_changed, Some(id))
    }

    pub fn exchange_add32(
        &mut self,
        address: u32,
        source: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory.exchange_add32(GuestAddress(address), source);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_helper(result)?;
        self.cpu_store_currentness(code_changed, None)
    }

    pub fn exchange_add_resident32(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        source: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory.exchange_add32(GuestAddress(address), source);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_helper(result)?;
        self.cpu_store_currentness(code_changed, Some(id))
    }

    pub fn exchange_add8(
        &mut self,
        address: u32,
        source: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory
            .exchange_add8(GuestAddress(address), source as u8)
            .map(u32::from);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_byte_exchange_helper(address, result)?;
        self.cpu_store_currentness(code_changed, None)
    }

    pub fn exchange_add_resident8(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        source: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory
            .exchange_add8(GuestAddress(address), source as u8)
            .map(u32::from);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_byte_exchange_helper(address, result)?;
        self.cpu_store_currentness(code_changed, Some(id))
    }

    pub fn exchange_add16(
        &mut self,
        address: u32,
        source: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory
            .exchange_add16(GuestAddress(address), source as u16)
            .map(u32::from);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_word_exchange_helper(address, result)?;
        self.cpu_store_currentness(code_changed, None)
    }

    pub fn exchange_add_resident16(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        source: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory
            .exchange_add16(GuestAddress(address), source as u16)
            .map(u32::from);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_word_exchange_helper(address, result)?;
        self.cpu_store_currentness(code_changed, Some(id))
    }

    pub fn bit_test_set32(&mut self, address: u32, bit: u32) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory.bit_test_set32(GuestAddress(address), bit);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_helper(result)?;
        self.cpu_store_currentness(code_changed, None)
    }

    pub fn bit_test_set_resident32(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        bit: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory.bit_test_set32(GuestAddress(address), bit);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_helper(result)?;
        self.cpu_store_currentness(code_changed, Some(id))
    }

    pub fn bit_test_reset32(
        &mut self,
        address: u32,
        bit: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory.bit_test_reset32(GuestAddress(address), bit);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_helper(result)?;
        self.cpu_store_currentness(code_changed, None)
    }

    pub fn bit_test_reset_resident32(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        bit: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory.bit_test_reset32(GuestAddress(address), bit);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_helper(result)?;
        self.cpu_store_currentness(code_changed, Some(id))
    }

    pub fn atomic_or_u32(&mut self, address: u32, mask: u32) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory.atomic_or_u32(GuestAddress(address), mask);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_helper(result)?;
        self.cpu_store_currentness(code_changed, None)
    }

    pub fn atomic_or_resident_u32(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        mask: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory.atomic_or_u32(GuestAddress(address), mask);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_helper(result)?;
        self.cpu_store_currentness(code_changed, Some(id))
    }

    pub fn atomic_and_u32(
        &mut self,
        address: u32,
        mask: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory.atomic_and_u32(GuestAddress(address), mask);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_helper(result)?;
        self.cpu_store_currentness(code_changed, None)
    }

    pub fn atomic_and_resident_u32(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        mask: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory.atomic_and_u32(GuestAddress(address), mask);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_helper(result)?;
        self.cpu_store_currentness(code_changed, Some(id))
    }

    pub fn atomic_xor_u32(
        &mut self,
        address: u32,
        mask: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory.atomic_xor_u32(GuestAddress(address), mask);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_helper(result)?;
        self.cpu_store_currentness(code_changed, None)
    }

    pub fn atomic_xor_resident_u32(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        mask: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let code_version = memory.code_validation_version();
        let result = memory.atomic_xor_u32(GuestAddress(address), mask);
        let code_changed = result.is_ok() && memory.code_validation_version() != code_version;
        self.write_helper(result)?;
        self.cpu_store_currentness(code_changed, Some(id))
    }

    fn write_word_exchange_helper(
        &mut self,
        address: u32,
        result: Result<u32, MemoryError>,
    ) -> Result<(), HostError> {
        let output = &mut self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE];
        encode_word_exchange_result(GuestAddress(address), result, output)
            .map_err(|_| HostError::Infrastructure)
    }

    fn write_byte_exchange_helper(
        &mut self,
        address: u32,
        result: Result<u32, MemoryError>,
    ) -> Result<(), HostError> {
        let output = &mut self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE];
        encode_byte_exchange_result(GuestAddress(address), result, output)
            .map_err(|_| HostError::Infrastructure)
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

#[cfg(test)]
#[path = "scalar/tests.rs"]
mod tests;
