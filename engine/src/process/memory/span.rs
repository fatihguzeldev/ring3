use crate::process::{EngineInstance, HostError, StoreCompletion};
use crate::{
    abi::{
        arena::{HELPER_OFFSET, TRANSFER_OFFSET},
        cpu_helper::{CpuMemoryError, MAX_CPU_TRANSFER, encode_cpu_result},
        memory_helper::HELPER_SIZE,
    },
    cpu::dbt::RegistryError,
    memory::{Access, GuestAddress},
};

impl EngineInstance {
    pub fn cpu_read(&mut self, address: u32, length: u32, alignment: u32) -> Result<(), HostError> {
        check_request(length, alignment)?;
        self.memory()?;
        let result = if address & (alignment - 1) != 0 {
            Err(CpuMemoryError::GeneralProtection(0))
        } else {
            let memory = self.memory.as_ref().ok_or(HostError::Closed)?;
            let output = &mut self.arena.as_mut().get_mut()
                [TRANSFER_OFFSET..TRANSFER_OFFSET + length as usize];
            memory
                .read(GuestAddress(address), output)
                .map_err(CpuMemoryError::Memory)
        };
        self.cpu_memory_result(length, result)
    }

    pub fn cpu_write(
        &mut self,
        address: u32,
        length: u32,
        alignment: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        self.cpu_write_bound(None, address, length, alignment, length)
    }

    pub fn cpu_write_resident(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        length: u32,
        alignment: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        self.cpu_write_bound(Some(id), address, length, alignment, length)
    }

    pub fn cpu_fxsave(&mut self, address: u32) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        self.cpu_write_bound(None, address, 512, 16, 464)
    }

    pub fn cpu_fxsave_resident(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        self.cpu_write_bound(Some(id), address, 512, 16, 464)
    }

    fn cpu_write_bound(
        &mut self,
        resident: Option<u64>,
        address: u32,
        length: u32,
        alignment: u32,
        written: u32,
    ) -> Result<StoreCompletion, HostError> {
        check_request(length, alignment)?;
        let input =
            &self.arena.as_ref().get_ref()[TRANSFER_OFFSET..TRANSFER_OFFSET + written as usize];
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let version = memory.code_validation_version();
        let result = if address & (alignment - 1) != 0 {
            Err(CpuMemoryError::GeneralProtection(0))
        } else {
            let preflight = if written == length {
                Ok(())
            } else {
                memory.check_span(GuestAddress(address), length as usize, Access::Write)
            };
            preflight
                .and_then(|()| memory.write(GuestAddress(address), input))
                .map_err(CpuMemoryError::Memory)
        };
        let changed = result.is_ok() && version != memory.code_validation_version();
        self.cpu_memory_result(length, result)?;
        self.cpu_store_currentness(changed, resident)
    }

    pub fn cpu_mask_write(
        &mut self,
        address: u32,
        length: u32,
        mask: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        self.cpu_mask_write_bound(None, address, length, mask)
    }

    pub fn cpu_mask_write_resident(
        &mut self,
        key: u64,
        id: u64,
        address: u32,
        length: u32,
        mask: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        self.cpu_mask_write_bound(Some(id), address, length, mask)
    }

    fn cpu_mask_write_bound(
        &mut self,
        resident: Option<u64>,
        address: u32,
        length: u32,
        mask: u32,
    ) -> Result<StoreCompletion, HostError> {
        if !matches!(length, 8 | 16) || mask >> length != 0 {
            return Err(HostError::InvalidRequest);
        }
        let input =
            &self.arena.as_ref().get_ref()[TRANSFER_OFFSET..TRANSFER_OFFSET + length as usize];
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let version = memory.code_validation_version();
        let result = memory
            .write_masked(GuestAddress(address), input, mask)
            .map_err(CpuMemoryError::Memory);
        let changed = result.is_ok() && version != memory.code_validation_version();
        self.cpu_memory_result(length, result)?;
        self.cpu_store_currentness(changed, resident)
    }

    pub(in crate::process) fn cpu_store_currentness(
        &self,
        changed: bool,
        resident: Option<u64>,
    ) -> Result<StoreCompletion, HostError> {
        if changed {
            let current = match resident {
                Some(id) => self.resident_bytes(id).map(|_| ()),
                None => self.artifact_bytes().map(|_| ()),
            };
            match current {
                Err(
                    HostError::CodeInvalidated
                    | HostError::Resident(RegistryError::CodeInvalidated),
                ) => return Ok(StoreCompletion::CodeInvalidated),
                Err(error) => return Err(error),
                Ok(()) => {}
            }
        }
        Ok(StoreCompletion::Complete)
    }

    fn cpu_memory_result(
        &mut self,
        length: u32,
        result: Result<(), CpuMemoryError>,
    ) -> Result<(), HostError> {
        encode_cpu_result(
            length,
            0,
            result,
            &mut self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)
    }
}

fn check_request(length: u32, alignment: u32) -> Result<(), HostError> {
    if length == 0 || length as usize > MAX_CPU_TRANSFER || !matches!(alignment, 1 | 8 | 16) {
        return Err(HostError::InvalidRequest);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::arena::TRANSFER_OFFSET;

    fn engine() -> EngineInstance {
        let mut engine = EngineInstance::new(3, 7).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        engine.map(0x2000, 1, 3).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
            .copy_from_slice(&[0xeb, 0xfe]);
        engine.upload(0x1000, 2).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
            .copy_from_slice(&[0x00, 0x10, 0, 0, 2, 0, 0, 0]);
        engine.compile(1).unwrap();
        engine
    }

    fn field(engine: &EngineInstance, offset: usize) -> u32 {
        u32::from_le_bytes(
            engine.arena()[HELPER_OFFSET + offset..HELPER_OFFSET + offset + 4]
                .try_into()
                .unwrap(),
        )
    }

    #[test]
    fn whole_span_read_fault_preserves_staged_input() {
        let mut engine = engine();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 16].fill(0xa5);
        engine.cpu_read(0x2ff8, 16, 1).unwrap();
        assert_eq!(field(&engine, 16), 1);
        assert_eq!(field(&engine, 28), 0x3000);
        assert_eq!(field(&engine, 36), 16);
        assert_eq!(
            &engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + 16],
            &[0xa5; 16]
        );
    }

    #[test]
    fn whole_span_write_fault_does_not_publish_first_page_or_version() {
        let mut engine = engine();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 16].fill(0xa5);
        let version = engine.memory().unwrap().code_validation_version();
        assert_eq!(
            engine.cpu_write(0x2ff8, 16, 1),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!(field(&engine, 16), 1);
        assert_eq!(field(&engine, 28), 0x3000);
        let mut bytes = [1; 8];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x2ff8), &mut bytes)
            .unwrap();
        assert_eq!(bytes, [0; 8]);
        assert_eq!(engine.memory().unwrap().code_validation_version(), version);
    }

    #[test]
    fn alignment_exception_precedes_unmapped_access() {
        let mut engine = engine();
        engine.cpu_read(0x4001, 16, 16).unwrap();
        assert_eq!(field(&engine, 16), 3);
        assert_eq!(field(&engine, 24), 0);
        engine.cpu_write(0x4001, 16, 16).unwrap();
        assert_eq!(field(&engine, 16), 3);
        assert_eq!(engine.cpu_read(0, 513, 1), Err(HostError::InvalidRequest));
    }

    #[test]
    fn fxsave_checks_all_512_bytes_before_writing_464() {
        let mut engine = engine();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 512].fill(0xa5);
        engine.cpu_fxsave(0x2e10).unwrap();
        assert_eq!(
            (field(&engine, 16), field(&engine, 28), field(&engine, 36)),
            (1, 0x3000, 512)
        );
        let mut prefix = [1; 464];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x2e10), &mut prefix)
            .unwrap();
        assert_eq!(prefix, [0; 464]);
        assert_eq!(
            &engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + 512],
            &[0xa5; 512]
        );
    }

    #[test]
    fn fxsave_preserves_software_tail_without_read_permission() {
        let mut engine = engine();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 512].fill(0x6b);
        engine.upload(0x2000, 512).unwrap();
        engine.protect(0x2000, 1, 2).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 512].fill(0xa5);
        engine.cpu_fxsave(0x2000).unwrap();
        assert_eq!((field(&engine, 16), field(&engine, 36)), (0, 512));
        engine.protect(0x2000, 1, 3).unwrap();
        let mut saved = [0; 512];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x2000), &mut saved)
            .unwrap();
        assert_eq!(&saved[..464], &[0xa5; 464]);
        assert_eq!(&saved[464..], &[0x6b; 48]);
    }

    #[test]
    fn maximum_split_read_and_smaller_prefix_preserve_all_other_arena_bytes() {
        let mut engine = engine();
        engine.map(0x3000, 1, 3).unwrap();
        let bytes: [u8; 512] = std::array::from_fn(|i| (i as u8).wrapping_mul(29).wrapping_add(7));
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 512].copy_from_slice(&bytes);
        engine.upload(0x2f00, 512).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4096].fill(0xa5);
        let mut expected = engine.arena().to_vec();
        let version = engine.memory().unwrap().code_validation_version();
        engine.cpu_read(0x2f00, 512, 16).unwrap();
        expected[TRANSFER_OFFSET..TRANSFER_OFFSET + 512].copy_from_slice(&bytes);
        expected[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE].copy_from_slice(&[
            0x52, 0x33, 0x4d, 0x48, 7, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0,
        ]);
        assert_eq!(engine.arena(), expected);
        assert_eq!(engine.memory().unwrap().code_validation_version(), version);

        engine.cpu_read(0x2ffd, 8, 1).unwrap();
        expected[TRANSFER_OFFSET..TRANSFER_OFFSET + 8].copy_from_slice(&bytes[253..261]);
        expected[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE].copy_from_slice(&[
            0x52, 0x33, 0x4d, 0x48, 7, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0, 0,
        ]);
        assert_eq!(engine.arena(), expected);
        assert_eq!(engine.memory().unwrap().code_validation_version(), version);
    }
}
