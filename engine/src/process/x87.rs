use crate::{
    abi::{
        arena::{HELPER_OFFSET, X87_OFFSET},
        cpu_helper::{CpuMemoryError, encode_cpu_result},
        memory_helper::HELPER_SIZE,
        x86::{X87_SIZE, decode_x87, encode_x87},
    },
    cpu::x86::{SegmentRegister, X87Error, X87Operation},
    memory::{Access, GuestAddress},
};

use super::instance::{EngineInstance, HostError, StoreCompletion};

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
        let segments = self.segment_state()?;
        let segment = if segment == 0 {
            SegmentRegister::Ds.index()
        } else {
            (segment - 1) as usize
        };
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
                state.data_pointer = address.wrapping_sub(segments.bases[segment]);
                state.data_selector = segments.selectors[segment];
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
