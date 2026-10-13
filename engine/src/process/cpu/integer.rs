use crate::process::{EngineInstance, HostError, StoreCompletion};
use crate::{
    abi::{
        arena::{HELPER_OFFSET, TRANSFER_OFFSET},
        cpu_helper::{CpuMemoryError, encode_cpu_result},
        memory_helper::HELPER_SIZE,
    },
    cpu::x86::atomic::{AtomicKind, evaluate},
    memory::GuestAddress,
};

impl EngineInstance {
    pub fn cpu_integer(
        &mut self,
        descriptor: u32,
        address: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        self.cpu_integer_bound(None, descriptor, address)
    }
    pub fn cpu_integer_resident(
        &mut self,
        key: u64,
        id: u64,
        descriptor: u32,
        address: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        self.cpu_integer_bound(Some(id), descriptor, address)
    }
    fn cpu_integer_bound(
        &mut self,
        resident: Option<u64>,
        descriptor: u32,
        address: u32,
    ) -> Result<StoreCompletion, HostError> {
        let arena = self.arena.as_ref().get_ref();
        let input: [u32; 6] = std::array::from_fn(|i| {
            u32::from_le_bytes(
                arena[TRANSFER_OFFSET + i * 4..TRANSFER_OFFSET + i * 4 + 4]
                    .try_into()
                    .unwrap(),
            )
        });
        if matches!(descriptor, 0x30001 | 0x30002) {
            return self.verify_segment(address, input, descriptor == 0x30002);
        }
        if matches!(descriptor, 0x30003 | 0x30004) {
            let kind = if descriptor == 0x30003 {
                crate::cpu::x86::SegmentQuery::AccessRights
            } else {
                crate::cpu::x86::SegmentQuery::Limit
            };
            return self.query_segment(address, input, kind);
        }
        if descriptor == 0x30000 {
            return self.adjust_privilege_memory(resident, address, input);
        }
        let mut output = input;
        if matches!(descriptor, 0x20000 | 0x20001) {
            self.virtual_tsc = self.virtual_tsc.wrapping_add(1);
            output[2] = self.virtual_tsc as u32;
            output[3] = (self.virtual_tsc >> 32) as u32;
            if descriptor == 0x20001 {
                // all guest threads share the current single virtual processor.
                output[5] = crate::cpu::x86::USER_MODE_ENVIRONMENT.tsc_aux();
            }
            self.integer_publish(0, output, Ok(()))?;
            return Ok(StoreCompletion::Complete);
        }
        let compare = descriptor == 0x10000;
        let width = if compare { 8 } else { (descriptor >> 8) & 15 };
        let kind = if compare {
            None
        } else {
            AtomicKind::decode(descriptor & 255)
        };
        if !compare && (kind.is_none() || !matches!(width, 1 | 2 | 4) || descriptor & !0xfff != 0) {
            return Err(HostError::InvalidRequest);
        }
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let version = memory.code_validation_version();
        let mut old = [0u8; 8];
        let result = memory
            .read(GuestAddress(address), &mut old[..width as usize])
            .and_then(|()| {
                let value = if compare {
                    let old = u64::from_le_bytes(old);
                    let expected = u64::from(input[2]) | (u64::from(input[3]) << 32);
                    output[1] = (input[1] & !0x40) | if old == expected { 0x40 } else { 0 };
                    if old == expected {
                        u64::from(input[4]) | (u64::from(input[5]) << 32)
                    } else {
                        output[2] = old as u32;
                        output[3] = (old >> 32) as u32;
                        old
                    }
                } else {
                    let result = evaluate(
                        kind.unwrap(),
                        width,
                        u32::from_le_bytes(old[..4].try_into().unwrap()),
                        input[0],
                        input[1],
                    )
                    .unwrap();
                    output[1] = result.flags;
                    u64::from(result.value)
                };
                memory.write(
                    GuestAddress(address),
                    &value.to_le_bytes()[..width as usize],
                )
            })
            .map_err(CpuMemoryError::Memory);
        let changed = result.is_ok() && version != memory.code_validation_version();
        self.integer_publish(width, output, result)?;
        self.cpu_store_currentness(changed, resident)
    }
    pub(super) fn integer_publish(
        &mut self,
        length: u32,
        output: [u32; 6],
        result: Result<(), CpuMemoryError>,
    ) -> Result<(), HostError> {
        let arena = self.arena.as_mut().get_mut();
        encode_cpu_result(
            length,
            0,
            result,
            &mut arena[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)?;
        if result.is_ok() {
            for (i, value) in output.into_iter().enumerate() {
                arena[TRANSFER_OFFSET + i * 4..TRANSFER_OFFSET + i * 4 + 4]
                    .copy_from_slice(&value.to_le_bytes());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> EngineInstance {
        let mut engine = EngineInstance::new(1, 0x43545343).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
            .copy_from_slice(&[0x0f, 0x31, 0xeb, 0]);
        engine.upload(0x1000, 4).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
            .copy_from_slice(&[0, 0x10, 0, 0, 4, 0, 0, 0]);
        engine.compile(1).unwrap();
        engine
    }

    fn transfer(engine: &EngineInstance, index: usize) -> u32 {
        u32::from_le_bytes(
            engine.arena()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
                .try_into()
                .unwrap(),
        )
    }

    #[test]
    fn timestamp_high_word_rollover_and_aux_share_one_counter() {
        let mut engine = engine();
        engine.virtual_tsc = u64::from(u32::MAX);
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + 20..TRANSFER_OFFSET + 24]
            .copy_from_slice(&u32::MAX.to_le_bytes());
        engine.cpu_integer(0x20001, 0).unwrap();
        assert_eq!(engine.virtual_tsc, 1 << 32);
        assert_eq!(
            (
                transfer(&engine, 2),
                transfer(&engine, 3),
                transfer(&engine, 5)
            ),
            (0, 1, 0)
        );
        engine.virtual_tsc = u64::MAX;
        engine.cpu_integer(0x20000, 0).unwrap();
        assert_eq!(engine.virtual_tsc, 0);
        assert_eq!((transfer(&engine, 2), transfer(&engine, 3)), (0, 0));
    }

    #[test]
    fn rejected_timestamp_requests_do_not_publish_or_consume_events() {
        let mut engine = engine();
        engine.virtual_tsc = 17;
        for descriptor in [0x20002, 0x20003, 0x40020001, u32::MAX] {
            let before = engine.arena().to_vec();
            assert_eq!(
                engine.cpu_integer(descriptor, 0),
                Err(HostError::InvalidRequest)
            );
            assert_eq!(engine.virtual_tsc, 17);
            assert_eq!(engine.arena(), before);
        }
        engine.close();
        let before = engine.arena().to_vec();
        assert_eq!(engine.cpu_integer(0x20001, 0), Err(HostError::Closed));
        assert_eq!(engine.virtual_tsc, 17);
        assert_eq!(engine.arena(), before);
    }
}
