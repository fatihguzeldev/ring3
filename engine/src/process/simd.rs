use super::{EngineInstance, HostError};
use crate::{
    abi::{
        arena::{HELPER_OFFSET, TRANSFER_OFFSET},
        cpu_helper::{CpuMemoryError, encode_cpu_result},
        memory_helper::HELPER_SIZE,
    },
    cpu::x86::simd::{SimdError, execute_descriptor},
};

impl EngineInstance {
    pub fn simd_execute(
        &mut self,
        descriptor: u32,
        operands: u32,
        immediate: u32,
        source_gpr: u32,
        destination_gpr: u32,
        flags: u32,
    ) -> Result<(), HostError> {
        self.guard(self.key, self.generation())?;
        self.simd_apply(
            descriptor,
            operands,
            immediate,
            source_gpr,
            destination_gpr,
            flags,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn simd_execute_resident(
        &mut self,
        key: u64,
        id: u64,
        descriptor: u32,
        operands: u32,
        immediate: u32,
        source_gpr: u32,
        destination_gpr: u32,
        flags: u32,
    ) -> Result<(), HostError> {
        self.guard_resident(key, id)?;
        self.simd_apply(
            descriptor,
            operands,
            immediate,
            source_gpr,
            destination_gpr,
            flags,
        )
    }

    fn simd_apply(
        &mut self,
        descriptor: u32,
        operands: u32,
        immediate: u32,
        source_gpr: u32,
        destination_gpr: u32,
        flags: u32,
    ) -> Result<(), HostError> {
        let result = execute_descriptor(
            self.arena.as_mut().get_mut(),
            descriptor,
            operands,
            immediate,
            source_gpr,
            destination_gpr,
            flags,
        );
        let packet = match result {
            Ok(value) => {
                let output = &mut self.arena.as_mut().get_mut()
                    [TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 520];
                output[..4].copy_from_slice(&value.gpr.to_le_bytes());
                output[4..].copy_from_slice(&value.flags.to_le_bytes());
                Ok(())
            }
            Err(SimdError::GeneralProtection(code)) => Err(CpuMemoryError::GeneralProtection(code)),
            Err(SimdError::FloatingPoint(mxcsr)) => Err(CpuMemoryError::SimdFloatingPoint(mxcsr)),
            Err(SimdError::X87FloatingPoint(status)) => Err(CpuMemoryError::FloatingPoint(status)),
            Err(SimdError::InvalidDescriptor) => return Err(HostError::InvalidRequest),
            Err(SimdError::InvalidState) => return Err(HostError::Infrastructure),
        };
        encode_cpu_result(
            0,
            0,
            packet,
            &mut self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE],
        )
        .map_err(|_| HostError::Infrastructure)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cpu::x86::{
            Register32, State32,
            simd::{SimdKind, SimdOperand, SimdOperation, SimdRegister},
        },
        memory::GuestAddress,
        process::{
            CallError,
            call::{ExtendedCpuState, PendingCall, PendingOwner},
            callback::{SuspendedCallback, SuspendedRecord},
        },
        windows::{CallFrame32, CallingConvention32},
    };

    fn engine() -> (EngineInstance, u64) {
        let mut engine = EngineInstance::new(2, 7).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        engine.map(0x2000, 1, 3).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0x1000), &[0xeb, 0xfe])
            .unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
            .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
        engine.compile(1).unwrap();
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
            .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
        let id = engine.compile_resident(1).unwrap().get();
        (engine, id)
    }
    fn descriptor() -> (u32, u32, u32) {
        SimdOperation {
            kind: SimdKind::Move,
            destination: SimdOperand::Register(SimdRegister::Xmm(0)),
            source: SimdOperand::Register(SimdRegister::Xmm(1)),
            vector_bytes: 16,
            lane_bytes: 16,
            memory_bytes: 0,
            immediate: None,
            aligned: false,
            scalar: false,
            implicit_address: None,
        }
        .descriptor()
    }

    #[test]
    fn simd_authority_pending_callback_stale_and_closed_precede_all_mutation() {
        let (mut engine, id) = engine();
        let (d, o, i) = descriptor();
        let mut state = State32::default();
        state.registers[Register32::Esp.index()] = 0x2000;
        let frame = CallFrame32::capture(
            engine.memory().unwrap(),
            state,
            CallingConvention32::Cdecl,
            0,
        )
        .unwrap();
        engine.pending_call = Some(PendingCall {
            token: 1,
            owner: PendingOwner::Resident(id),
            frame,
            state: engine.arena()[..56].try_into().unwrap(),
            exit: engine.arena()[56..96].try_into().unwrap(),
            extended: ExtendedCpuState::capture(engine.arena()).unwrap(),
        });
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.simd_execute(d, o, i, 0, 0, 2),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(
            engine.simd_execute_resident(engine.key, id, d, o, i, 0, 0, 2),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(engine.arena(), before);
        let outer = engine.pending_call.take().unwrap();
        engine.callback = Some(SuspendedCallback {
            outer,
            record: SuspendedRecord::Resident {
                record: crate::abi::resident_callback::ResidentCallbackRecord32 {
                    token: 2,
                    outer_token: 1,
                    phase: 1,
                    outcome: 0,
                    entry_pc: 0x1000,
                    entry_esp: 0x2000,
                    return_pc: 0x1000,
                    return_id: 7,
                    stack_words: 0,
                    result: 0,
                    outer_unit_id: id,
                    callback_unit_id: id,
                },
                authorized: false,
                active_unit_id: id,
            },
        });
        assert_eq!(
            engine.simd_execute(d, o, i, 0, 0, 2),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(
            engine.simd_execute_resident(engine.key, id, d, o, i, 0, 0, 2),
            Err(HostError::Call(CallError::Busy))
        );
        assert_eq!(engine.arena(), before);
        engine.callback = None;
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0x1000), &[0xeb, 0xfe])
            .unwrap();
        assert_eq!(
            engine.simd_execute(d, o, i, 0, 0, 2),
            Err(HostError::CodeInvalidated)
        );
        assert_eq!(
            engine.simd_execute_resident(engine.key, id, d, o, i, 0, 0, 2),
            Err(HostError::Resident(
                crate::cpu::dbt::RegistryError::CodeInvalidated
            ))
        );
        assert_eq!(engine.arena(), before);
        engine.close();
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.simd_execute(d, o, i, 0, 0, 2),
            Err(HostError::Closed)
        );
        assert_eq!(engine.arena(), before);
    }

    #[test]
    fn simd_invalid_descriptor_and_wrong_resident_leave_whole_arena_neutral() {
        let (mut engine, id) = engine();
        let (d, o, i) = descriptor();
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.simd_execute(d | 0x8000_0000, o, i, 0, 0, 2),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(
            engine.simd_execute_resident(engine.key + 1, id, d, o, i, 0, 0, 2),
            Err(HostError::InvalidArtifact)
        );
        assert_eq!(engine.arena(), before);
        engine
            .simd_execute_resident(engine.key, id, d, o, i, 0, 0, 0xcd7)
            .unwrap();
        assert_eq!(
            &engine.arena()[TRANSFER_OFFSET + 512..TRANSFER_OFFSET + 520],
            &[0, 0, 0, 0, 0xd7, 0x0c, 0, 0]
        );
    }
}
