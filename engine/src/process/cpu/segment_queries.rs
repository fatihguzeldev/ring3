use crate::{
    abi::cpu_helper::CpuMemoryError,
    cpu::x86::{SegmentAccess, SegmentRegister},
    memory::{Access, GuestAddress},
    process::{EngineInstance, HostError, StoreCompletion},
};

impl EngineInstance {
    pub(super) fn adjust_privilege_memory(
        &mut self,
        resident: Option<u64>,
        address: u32,
        input: [u32; 6],
    ) -> Result<StoreCompletion, HostError> {
        let segment = match input[3] {
            0 => SegmentRegister::Es,
            1 => SegmentRegister::Cs,
            2 => SegmentRegister::Ss,
            3 => SegmentRegister::Ds,
            4 => SegmentRegister::Fs,
            5 => SegmentRegister::Gs,
            _ => return Err(HostError::InvalidRequest),
        };
        let context = self.segment_context()?;
        if context.state.bases[segment.index()].wrapping_add(input[2]) != address {
            return Err(HostError::InvalidRequest);
        }
        let mut output = input;
        if context.caches[segment.index()]
            .check_span(input[2], 2, SegmentAccess::Write)
            .is_err()
        {
            let fault = if segment == SegmentRegister::Ss {
                CpuMemoryError::StackSegment(0)
            } else {
                CpuMemoryError::GeneralProtection(0)
            };
            self.integer_publish(2, output, Err(fault))?;
            return Ok(StoreCompletion::Complete);
        }
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let version = memory.code_validation_version();
        let mut bytes = [0; 2];
        let result = memory
            .check_span(GuestAddress(address), 2, Access::Write)
            .and_then(|()| memory.read(GuestAddress(address), &mut bytes))
            .and_then(|()| {
                let old = u16::from_le_bytes(bytes);
                let source_rpl = (input[0] & 3) as u16;
                let adjusted = old & 3 < source_rpl;
                output[1] = (input[1] & !0x40) | if adjusted { 0x40 } else { 0 };
                if adjusted {
                    memory.write(
                        GuestAddress(address),
                        &((old & !3) | source_rpl).to_le_bytes(),
                    )
                } else {
                    Ok(())
                }
            })
            .map_err(CpuMemoryError::Memory);
        let changed = result.is_ok() && version != memory.code_validation_version();
        self.integer_publish(2, output, result)?;
        self.cpu_store_currentness(changed, resident)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        abi::{
            arena::{HELPER_OFFSET, SEGMENTS_OFFSET, TRANSFER_OFFSET},
            x86::{SEGMENTS_SIZE, encode_segment_context},
        },
        cpu::x86::{SegmentCache, SegmentDescriptor, SegmentDescriptorKind},
    };

    fn engine() -> EngineInstance {
        let mut engine = EngineInstance::new(4, 0x4152504c).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        engine.map(0x5000, 1, 3).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
            .copy_from_slice(&[0x63, 0x0f, 0xeb, 0xfe]);
        engine.upload(0x1000, 4).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
            .copy_from_slice(&[0, 0x10, 0, 0, 4, 0, 0, 0]);
        engine.compile(1).unwrap();
        engine
    }

    fn input(engine: &mut EngineInstance, source: u32, offset: u32, segment: u32) {
        for (index, value) in [source, 0x24_4ed7, offset, segment, 0xaabb_ccdd, 0x1122_3344]
            .into_iter()
            .enumerate()
        {
            engine.arena_mut().unwrap()
                [TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
                .copy_from_slice(&value.to_le_bytes());
        }
    }

    fn field(engine: &EngineInstance, at: usize) -> u32 {
        u32::from_le_bytes(engine.arena()[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn no_adjustment_still_checks_write_authority_and_faults_publish_no_payload() {
        let mut engine = engine();
        engine.write32(0x5010, 0xaabb_1123).unwrap();
        engine.protect(0x5000, 1, 1).unwrap();
        input(&mut engine, 0, 0x5010, 3);
        let before = engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + 24].to_vec();
        assert_eq!(
            engine.cpu_integer(0x30000, 0x5010),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!(
            &engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + 24],
            before
        );
        assert_eq!(field(&engine, HELPER_OFFSET + 16), 1);
        assert_eq!(field(&engine, HELPER_OFFSET + 24), 2);
        assert_eq!(field(&engine, HELPER_OFFSET + 28), 0x5010);
        assert_eq!(field(&engine, HELPER_OFFSET + 32), 2);
        assert_eq!(field(&engine, HELPER_OFFSET + 36), 2);
        engine.protect(0x5000, 1, 3).unwrap();
        let version = engine.memory().unwrap().code_validation_version();
        engine.cpu_integer(0x30000, 0x5010).unwrap();
        assert_eq!(field(&engine, TRANSFER_OFFSET + 4), 0x24_4e97);
        assert_eq!(engine.memory().unwrap().code_validation_version(), version);
    }

    #[test]
    fn cached_readonly_or_limit_refusal_precedes_page_lookup() {
        let mut engine = engine();
        engine
            .configure_segment_descriptor(SegmentDescriptor {
                selector: 0x43,
                base: 0,
                dpl: 3,
                present: true,
                kind: SegmentDescriptorKind::Data { writable: false },
            })
            .unwrap();
        engine.segment_load(3, 0x43).unwrap();
        engine
            .configure_segment_descriptor(SegmentDescriptor {
                selector: 0x43,
                base: 0,
                dpl: 3,
                present: true,
                kind: SegmentDescriptorKind::Data { writable: true },
            })
            .unwrap();
        input(&mut engine, 3, 0x7000, 3);
        engine.cpu_integer(0x30000, 0x7000).unwrap();
        assert_eq!(field(&engine, HELPER_OFFSET + 16), 3);
        assert_eq!(field(&engine, HELPER_OFFSET + 24), 0);
        let mut context = engine.segment_context().unwrap();
        context.caches[2] = SegmentCache::from_bits(0xf230_7000).unwrap();
        let mut record = [0; SEGMENTS_SIZE];
        encode_segment_context(&context, &mut record).unwrap();
        engine.arena_mut().unwrap()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE]
            .copy_from_slice(&record);
        input(&mut engine, 3, 0x7000, 2);
        engine.cpu_integer(0x30000, 0x7000).unwrap();
        assert_eq!(field(&engine, HELPER_OFFSET + 16), 7);
        assert_eq!(field(&engine, HELPER_OFFSET + 24), 0);
    }

    #[test]
    fn cross_page_preflight_is_atomic_and_only_low_rpl_bits_change_after_repair() {
        let mut engine = engine();
        engine.write32(0x5ffc, 0x0012_3456).unwrap();
        input(&mut engine, 3, 0x5fff, 3);
        engine.cpu_integer(0x30000, 0x5fff).unwrap();
        assert_eq!(field(&engine, HELPER_OFFSET + 28), 0x6000);
        let mut first = [0];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x5fff), &mut first)
            .unwrap();
        assert_eq!(first, [0]);
        engine.map(0x6000, 1, 3).unwrap();
        engine.write32(0x6000, 0xaabb_cc55).unwrap();
        engine.cpu_integer(0x30000, 0x5fff).unwrap();
        let mut result = [0; 2];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x5fff), &mut result)
            .unwrap();
        assert_eq!(result, [3, 0x55]);
        assert_eq!(field(&engine, TRANSFER_OFFSET + 4), 0x24_4ed7);
    }

    #[test]
    fn invalid_helper_descriptor_segment_or_linear_address_is_neutral() {
        let mut engine = engine();
        for (descriptor, offset, segment, address) in [
            (0x30001, 0x5010, 3, 0x5010),
            (0x30000, 0x5010, 6, 0x5010),
            (0x30000, 0x5010, 3, 0x5011),
        ] {
            input(&mut engine, 3, offset, segment);
            let before = engine.arena().to_vec();
            assert_eq!(
                engine.cpu_integer(descriptor, address),
                Err(HostError::InvalidRequest)
            );
            assert_eq!(engine.arena(), before);
        }
    }
}
