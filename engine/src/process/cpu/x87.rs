use crate::{
    abi::{
        arena::{HELPER_OFFSET, X87_OFFSET},
        cpu_helper::{CpuMemoryError, encode_cpu_result},
        memory_helper::HELPER_SIZE,
        x86::{X87_SIZE, decode_x87, encode_x87},
    },
    cpu::x86::{SegmentAccess, SegmentRegister, X87Error, X87Operation},
    memory::{Access, GuestAddress},
};

use crate::process::instance::{EngineInstance, HostError, StoreCompletion};

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
        self.guard(self.key, self.generation())?;
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
        self.guard_resident(key, id)?;
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
        let segment = (waiting >> 8) & 7;
        if opcode > 0x7ff || waiting & !0x701 != 0 || segment > 6 {
            return Err(HostError::InvalidRequest);
        }
        let mut state = decode_x87(&self.arena[X87_OFFSET..X87_OFFSET + X87_SIZE])
            .map_err(|_| HostError::Infrastructure)?;
        let length = operation.read_size().max(operation.write_size()) as u32;
        if waiting & 1 != 0 && state.status & 0x3f & !state.control != 0 {
            state.status |= 0x8080;
            let mut record = [0; X87_SIZE];
            encode_x87(&state, &mut record).map_err(|_| HostError::Infrastructure)?;
            self.arena[X87_OFFSET..X87_OFFSET + X87_SIZE].copy_from_slice(&record);
            return self.x87_packet(
                length,
                0,
                Err(CpuMemoryError::FloatingPoint(state.status)),
                false,
                resident,
            );
        }
        // waiting x87 exceptions precede operand authority, including null selectors.
        let context = self.segment_context()?;
        let segments = context.state;
        let segment = if segment == 0 {
            SegmentRegister::Ds
        } else {
            [
                SegmentRegister::Es,
                SegmentRegister::Cs,
                SegmentRegister::Ss,
                SegmentRegister::Ds,
                SegmentRegister::Fs,
                SegmentRegister::Gs,
            ][(segment - 1) as usize]
        };
        let offset = address.wrapping_sub(segments.bases[segment.index()]);
        for (width, access) in [
            (operation.read_size(), SegmentAccess::Read),
            (operation.write_size(), SegmentAccess::Write),
        ] {
            if width != 0
                && let Err(error) = super::segments::check_cached_span(
                    &context,
                    segment,
                    offset,
                    width as u32,
                    access,
                )
            {
                return self.x87_packet(length, 0, Err(error), false, resident);
            }
        }
        let mut input = [0; 108];
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        if operation.read_size() != 0
            && let Err(error) =
                memory.read(GuestAddress(address), &mut input[..operation.read_size()])
        {
            return self.x87_packet(
                length,
                0,
                Err(CpuMemoryError::Memory(error)),
                false,
                resident,
            );
        }
        if operation.write_size() != 0
            && let Err(error) =
                memory.check_span(GuestAddress(address), operation.write_size(), Access::Write)
        {
            return self.x87_packet(
                length,
                0,
                Err(CpuMemoryError::Memory(error)),
                false,
                resident,
            );
        }
        if operation.is_computation() {
            state.instruction_pointer = pc;
            state.opcode = opcode as u16;
            state.code_selector = segments.selectors[SegmentRegister::Cs.index()];
            if length != 0 {
                state.data_pointer = offset;
                state.data_selector = segments.selectors[segment.index()];
            }
        }
        let outcome = state
            .evaluate(operation, &input[..operation.read_size()], flags)
            .map_err(|error| {
                if error == X87Error::NumericalFailure {
                    HostError::Infrastructure
                } else {
                    HostError::InvalidRequest
                }
            })?;
        let mut record = [0; X87_SIZE];
        encode_x87(&outcome.state, &mut record).map_err(|_| HostError::Infrastructure)?;
        let version = memory.code_validation_version();
        if outcome.memory_len != 0
            && let Err(error) =
                memory.write(GuestAddress(address), &outcome.memory[..outcome.memory_len])
        {
            return self.x87_packet(
                length,
                0,
                Err(CpuMemoryError::Memory(error)),
                false,
                resident,
            );
        }
        let changed = version != memory.code_validation_version();
        self.arena[X87_OFFSET..X87_OFFSET + X87_SIZE].copy_from_slice(&record);
        self.x87_packet(
            length,
            outcome.eflags.unwrap_or(flags),
            Ok(()),
            changed,
            resident,
        )
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
