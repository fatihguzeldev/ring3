use crate::{
    cpu::x86::{
        CpuMemoryPort32, CpuPagingError32, CpuReadPort32, PagingAccess32, SegmentAccess,
        SegmentContext, SegmentRegister, State32, X87Operation, X87State,
        avx::VectorState,
        ir::{AddressSize, EffectiveAddress, Operation},
        simd::{
            self as simd_semantics, SimdError, SimdKind, SimdOperand, SimdOperation, SimdRegister,
        },
    },
    math::NumericalWorkspace,
    memory::{AddressSpace, GuestAddress},
};

mod avx;
mod memory;
mod simd;
mod x87;
mod xstate;

#[derive(Default)]
pub(crate) struct NumericState {
    pub(crate) x87: X87State,
    pub(crate) vectors: VectorState,
    workspace: NumericalWorkspace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NumericOutcome {
    pub(crate) code_invalidated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NumericError {
    Unsupported,
    Memory {
        error: CpuPagingError32,
        width: usize,
    },
    Cpu {
        vector: u32,
        detail: u32,
    },
    Infrastructure,
}

impl NumericError {
    fn memory(error: CpuPagingError32, width: usize) -> Self {
        Self::Memory { error, width }
    }

    fn segment(segment: SegmentRegister) -> Self {
        Self::Cpu {
            vector: if segment == SegmentRegister::Ss {
                12
            } else {
                13
            },
            detail: 0,
        }
    }

    fn simd(error: SimdError) -> Self {
        match error {
            SimdError::GeneralProtection(detail) => Self::Cpu { vector: 13, detail },
            SimdError::FloatingPoint(detail) => Self::Cpu { vector: 19, detail },
            SimdError::X87FloatingPoint(status) => Self::Cpu {
                vector: 16,
                detail: u32::from(status),
            },
            SimdError::InvalidDescriptor | SimdError::InvalidState => Self::Infrastructure,
        }
    }
}

fn offset(address: EffectiveAddress, state: &State32) -> u32 {
    let register = |register: crate::cpu::x86::Register32| state.registers[register.index()];
    let value = address
        .displacement
        .wrapping_add(address.base.map_or(0, register))
        .wrapping_add(address.index.map_or(0, |index| {
            register(index).wrapping_mul(u32::from(address.scale))
        }));
    match address.address_size {
        AddressSize::Bits16 => value & 0xffff,
        AddressSize::Bits32 => value,
    }
}

fn span(
    segments: &SegmentContext,
    segment: SegmentRegister,
    offset: u32,
    width: usize,
    access: SegmentAccess,
) -> Result<u32, NumericError> {
    segments.caches[segment.index()]
        .check_span(offset, width as u32, access)
        .map_err(|_| NumericError::segment(segment))?;
    Ok(segments.state.bases[segment.index()].wrapping_add(offset))
}

fn operand(
    segments: &SegmentContext,
    address: EffectiveAddress,
    state: &State32,
    width: usize,
    access: SegmentAccess,
) -> Result<u32, NumericError> {
    span(
        segments,
        address.resolved_segment(),
        offset(address, state),
        width,
        access,
    )
}

impl NumericState {
    pub(crate) fn execute(
        &mut self,
        memory: &mut AddressSpace,
        scalar: &mut State32,
        segments: &SegmentContext,
        operation: &Operation,
        next_pc: u32,
    ) -> Result<NumericOutcome, NumericError> {
        let before = memory.code_validation_version();
        match *operation {
            Operation::X87 {
                operation,
                address,
                waiting,
                opcode,
            } => self.x87(
                memory, scalar, segments, operation, address, waiting, opcode,
            )?,
            Operation::Simd(operation) => self.simd(memory, scalar, segments, operation)?,
            Operation::Avx(operation) => self.avx(memory, scalar, segments, operation)?,
            Operation::XState(operation) => self.xstate(memory, scalar, segments, operation)?,
            Operation::InitializeX87 => self.x87(
                memory,
                scalar,
                segments,
                X87Operation::Initialize,
                None,
                false,
                0,
            )?,
            Operation::ClearX87Exceptions => self.x87(
                memory,
                scalar,
                segments,
                X87Operation::ClearExceptions,
                None,
                false,
                0,
            )?,
            Operation::X87StatusToAx => self.x87(
                memory,
                scalar,
                segments,
                X87Operation::StatusToAx,
                None,
                false,
                0,
            )?,
            Operation::X87StatusToMemory { address } => self.x87(
                memory,
                scalar,
                segments,
                X87Operation::StoreStatus,
                Some(address),
                false,
                0,
            )?,
            Operation::X87ControlToMemory { address } => self.x87(
                memory,
                scalar,
                segments,
                X87Operation::StoreControl,
                Some(address),
                false,
                0,
            )?,
            _ => return Err(NumericError::Unsupported),
        }
        scalar.eip = next_pc;
        Ok(NumericOutcome {
            code_invalidated: memory.code_validation_version() != before,
        })
    }
}

#[cfg(test)]
mod tests;
