use crate::{
    abi::{
        arena::{HELPER_OFFSET, X87_OFFSET},
        cpu_helper::{CpuMemoryError, encode_cpu_result},
        memory_helper::HELPER_SIZE,
        x86::{X87_SIZE, decode_x87, encode_x87},
    },
    cpu::x86::{
        CpuMemoryPort32, CpuReadPort32, PagingAccess32, SegmentAccess, SegmentRegister, X87Error,
        X87Operation,
    },
    memory::GuestAddress,
};

use crate::process::instance::{EngineInstance, HostError, StoreCompletion};

mod pair;
mod step;

impl EngineInstance {
    pub fn x87_op(
        &mut self,
        operation: u32,
        address: u32,
        flags: u32,
        pc: u32,
        opcode: u32,
        waiting: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        self.x87_apply(None, operation, address, flags, pc, opcode, waiting)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn x87_resident_op(
        &mut self,
        key: u64,
        id: u64,
        operation: u32,
        address: u32,
        flags: u32,
        pc: u32,
        opcode: u32,
        waiting: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        self.x87_apply(Some(id), operation, address, flags, pc, opcode, waiting)
    }

    #[allow(clippy::too_many_arguments)]
    fn x87_apply(
        &mut self,
        resident: Option<u64>,
        word: u32,
        address: u32,
        flags: u32,
        pc: u32,
        opcode: u32,
        waiting: u32,
    ) -> Result<StoreCompletion, HostError> {
        let operation = X87Operation::decode(word).ok_or(HostError::InvalidRequest)?;
        let request = step::X87Request::new(operation, address, pc, opcode, waiting)?;
        let mut state = decode_x87(&self.arena[X87_OFFSET..X87_OFFSET + X87_SIZE])
            .map_err(|_| HostError::Infrastructure)?;
        let mut output = None;
        let outcome = self.x87_step(&mut state, &mut None, request, flags, &mut output)?;
        if outcome.result.is_err() || outcome.memory_len == 0 {
            if outcome.publish_state {
                encode_x87(&state, &mut self.arena[X87_OFFSET..X87_OFFSET + X87_SIZE])
                    .map_err(|_| HostError::Infrastructure)?;
            }
            return self.x87_packet(
                request.length,
                outcome.value,
                outcome.result,
                false,
                resident,
            );
        }
        let mut record = [0; X87_SIZE];
        encode_x87(&state, &mut record).map_err(|_| HostError::Infrastructure)?;
        let version = self.memory.as_ref().unwrap().code_validation_version();
        if let Err(error) = self.cpu_memory_port32().write(
            GuestAddress(address),
            &output.as_ref().unwrap()[..outcome.memory_len],
        ) {
            return self.x87_packet(
                request.length,
                0,
                Err(CpuMemoryError::from(error)),
                false,
                resident,
            );
        }
        let changed = version != self.memory.as_ref().unwrap().code_validation_version();
        self.arena[X87_OFFSET..X87_OFFSET + X87_SIZE].copy_from_slice(&record);
        self.x87_packet(request.length, outcome.value, Ok(()), changed, resident)
    }

    fn x87_packet(
        &mut self,
        length: u32,
        value: u32,
        result: Result<(), CpuMemoryError>,
        changed: bool,
        resident: Option<u64>,
    ) -> Result<StoreCompletion, HostError> {
        encode_cpu_result(
            length,
            value,
            result,
            &mut self.arena[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)?;
        self.cpu_store_currentness(changed, resident)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        abi::{
            arena::{SEGMENTS_OFFSET, TRANSFER_OFFSET},
            x86::{SEGMENTS_SIZE, encode_segment_context},
        },
        cpu::x86::{SegmentCache, X87Format, X87State},
    };

    mod publication;

    fn engine() -> EngineInstance {
        let mut engine = EngineInstance::new(3, 0x783837).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        engine.map(0x5000, 1, 3).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
            .copy_from_slice(&[0xeb, 0]);
        engine.upload(0x1000, 2).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
            .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
        engine.compile(1).unwrap();
        engine
    }

    #[test]
    fn fyl2x_captured_single_precision_control_completes_through_an_authorized_helper() {
        let mut engine = engine();
        let mut observed = X87State {
            control: 0x007f,
            status: 0x3020,
            tag: 0x0fff,
            opcode: 0x0502,
            instruction_pointer: 0x7802c309,
            data_pointer: 0x7e0ffca0,
            code_selector: 0x1b,
            data_selector: 0x23,
            ..X87State::default()
        };
        observed.registers[5] = [0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f];
        observed.registers[6] = [0, 0, 0, 0, 0, 0xc2, 0xc0, 0xc0, 0xf8, 0x3f];
        observed.registers[7] = [0, 0, 0, 0, 0, 0x63, 0x27, 0xf6, 0xfe, 0x3f];
        encode_x87(
            &observed,
            &mut engine.arena.as_mut().get_mut()[X87_OFFSET..X87_OFFSET + X87_SIZE],
        )
        .unwrap();
        engine
            .guard_material(engine.key, engine.generation())
            .unwrap();
        let before = engine.arena().to_vec();
        let version = engine.memory().unwrap().transaction_version();
        let result = engine.x87_op(34, 0, 0x46, 0x78028f76, 0x1f1, 1);
        if result.is_err() {
            assert_eq!(engine.arena(), before);
            assert_eq!(engine.memory().unwrap().transaction_version(), version);
        }
        assert_eq!(result, Ok(StoreCompletion::Complete));
        let actual = decode_x87(&engine.arena()[X87_OFFSET..X87_OFFSET + X87_SIZE]).unwrap();
        assert_eq!(actual.control, observed.control);
        assert_eq!(actual.top(), 7);
        assert_eq!(actual.status, 0x3820);
        assert_eq!(
            actual.stack(0).unwrap(),
            crate::cpu::x86::Ext80([0x23, 0xfe, 0xb3, 0xf6, 0xa9, 0x47, 0x36, 0xc5, 0x01, 0xc0,])
        );
        assert!(actual.stack(1).is_none());
        assert_eq!(actual.instruction_pointer, 0x78028f76);
        assert_eq!(actual.opcode, 0x1f1);
        assert_eq!(engine.memory().unwrap().transaction_version(), version);
    }

    fn cache(engine: &mut EngineInstance, segment: SegmentRegister, bits: u32) {
        let mut context = engine.segment_context().unwrap();
        context.caches[segment.index()] = SegmentCache::from_bits(bits).unwrap();
        encode_segment_context(
            &context,
            &mut engine.arena_mut().unwrap()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE],
        )
        .unwrap();
    }

    fn expected_packet(before: &[u8], status: u32, detail: u32, width: u32) -> Vec<u8> {
        let mut expected = before.to_vec();
        let packet = &mut expected[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE];
        packet.fill(0);
        packet[..4].copy_from_slice(b"R3MH");
        for (offset, value) in [
            (4, 0x10007_u32),
            (8, 40),
            (16, status),
            (24, detail),
            (36, width),
        ] {
            packet[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        expected
    }

    #[test]
    fn cached_x87_read_span_fault_is_fp_and_ram_neutral() {
        for (format, width) in [
            (X87Format::Float32, 4),
            (X87Format::Float64, 8),
            (X87Format::Extended80, 10),
        ] {
            let mut engine = engine();
            cache(
                &mut engine,
                SegmentRegister::Ds,
                0xf230_0000 | (0x5010 + width - 2),
            );
            let before = engine.arena().to_vec();
            let version = engine.memory().unwrap().code_validation_version();
            assert_eq!(
                engine.x87_op(
                    X87Operation::Load(format).encode(),
                    0x5010,
                    0x244cd7,
                    0x1000,
                    0,
                    0x401
                ),
                Ok(StoreCompletion::Complete)
            );
            assert_eq!(engine.arena(), expected_packet(&before, 3, 0, width));
            assert_eq!(engine.memory().unwrap().code_validation_version(), version);
        }
    }

    #[test]
    fn nonwaiting_x87_write_authority_precedes_unmapped_ram_and_reset() {
        let mut engine = engine();
        cache(&mut engine, SegmentRegister::Ds, 0xf31f_ffff);
        let pending = X87State {
            control: 0x37e,
            status: 1,
            ..X87State::default()
        };
        encode_x87(
            &pending,
            &mut engine.arena_mut().unwrap()[X87_OFFSET..X87_OFFSET + X87_SIZE],
        )
        .unwrap();
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.x87_op(
                X87Operation::Save { operand16: false }.encode(),
                0x7000,
                0x244cd7,
                0x1000,
                0,
                0x400
            ),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!(engine.arena(), expected_packet(&before, 3, 0, 108));
    }

    #[test]
    fn waiting_x87_mf_precedes_cached_segment_and_nonwaiting_ss_fault_is_precise() {
        let mut engine = engine();
        cache(&mut engine, SegmentRegister::Ss, 0xf230_5000);
        let pending = X87State {
            control: 0x37e,
            status: 1,
            ..X87State::default()
        };
        encode_x87(
            &pending,
            &mut engine.arena_mut().unwrap()[X87_OFFSET..X87_OFFSET + X87_SIZE],
        )
        .unwrap();
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.x87_op(
                X87Operation::Load(X87Format::Extended80).encode(),
                0x7000,
                0x244cd7,
                0x1000,
                0,
                0x301
            ),
            Ok(StoreCompletion::Complete)
        );
        let mut expected = expected_packet(&before, 4, 0x8081, 10);
        expected[X87_OFFSET + 18..X87_OFFSET + 20].copy_from_slice(&0x8081_u16.to_le_bytes());
        assert_eq!(engine.arena(), expected);
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.x87_op(
                X87Operation::Save { operand16: true }.encode(),
                0x7000,
                0x244cd7,
                0x1000,
                0,
                0x300
            ),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!(engine.arena(), expected_packet(&before, 7, 0, 94));
    }
}

#[cfg(test)]
#[path = "x87/paging.rs"]
mod paging_tests;
