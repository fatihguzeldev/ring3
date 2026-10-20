use crate::{
    abi::{
        arena::{SIMD_OFFSET, X87_OFFSET, YMM_OFFSET},
        cpu_helper::CpuMemoryError,
        x86::{
            SIMD_SIZE, X87_SIZE, YMM_SIZE, decode_simd, decode_x87, decode_ymm, encode_simd,
            encode_x87, encode_ymm,
        },
    },
    cpu::{
        core::{Context32, NativeInstruction, StopReason},
        x86::{
            CpuPagingError32, SegmentContext, X87Operation, X87State, avx::VectorState,
            ir::Operation, simd::ymm::YmmUpperState,
        },
    },
    memory::{Access, FaultReason, GuestAddress, MemoryError, MemoryFault},
    process::{EngineInstance, HostError},
};

#[path = "numeric/avx.rs"]
mod avx;
#[path = "numeric/memory.rs"]
mod memory;
#[path = "numeric/processor.rs"]
mod processor;
#[path = "numeric/simd.rs"]
mod simd;
#[path = "numeric/stack.rs"]
mod stack;
#[path = "numeric/x87.rs"]
mod x87;
#[path = "numeric/xstate.rs"]
mod xstate;

type NumericResult<T> = Result<T, Failure>;

enum Failure {
    Memory(CpuMemoryError, usize),
    Cpu(u32, u32),
    Infrastructure,
}

impl Failure {
    fn paging(error: CpuPagingError32, width: usize) -> Self {
        Self::Memory(error.into(), width)
    }

    fn publish(self, context: &mut Context32) -> u32 {
        match self {
            Self::Infrastructure => StopReason::Infrastructure as u32,
            Self::Cpu(vector, detail) => {
                context.set_word(116, vector);
                context.set_word(48, detail);
                context.set_word(52, 0);
                StopReason::CpuFault as u32
            }
            Self::Memory(CpuMemoryError::Memory(MemoryError::Fault(fault)), width)
            | Self::Memory(
                CpuMemoryError::Paging(CpuPagingError32::Memory(MemoryError::Fault(fault))),
                width,
            ) => memory_fault(context, fault, width),
            Self::Memory(CpuMemoryError::Paging(CpuPagingError32::PageFault(fault)), width) => {
                memory_fault(
                    context,
                    MemoryFault {
                        address: fault.linear(),
                        access: match fault.access() {
                            crate::cpu::x86::PagingAccess32::Read => Access::Read,
                            crate::cpu::x86::PagingAccess32::Write => Access::Write,
                            crate::cpu::x86::PagingAccess32::Execute => Access::Execute,
                        },
                        reason: if fault.error_code() & 1 == 0 {
                            FaultReason::Unmapped
                        } else {
                            FaultReason::Permission
                        },
                    },
                    width,
                )
            }
            Self::Memory(CpuMemoryError::GeneralProtection(code), _) => {
                Self::Cpu(13, code).publish(context)
            }
            Self::Memory(CpuMemoryError::StackSegment(code), _) => {
                Self::Cpu(12, code).publish(context)
            }
            Self::Memory(CpuMemoryError::SegmentNotPresent(code), _) => {
                Self::Cpu(11, code).publish(context)
            }
            Self::Memory(CpuMemoryError::FloatingPoint(status), _) => {
                Self::Cpu(16, u32::from(status)).publish(context)
            }
            Self::Memory(CpuMemoryError::SimdFloatingPoint(mxcsr), _) => {
                Self::Cpu(19, mxcsr).publish(context)
            }
            Self::Memory(_, _) => StopReason::Infrastructure as u32,
        }
    }
}

fn memory_fault(context: &mut Context32, fault: MemoryFault, width: usize) -> u32 {
    let access = match fault.access {
        Access::Read => 1,
        Access::Write => 2,
        Access::Execute => 4,
    };
    let reason = match fault.reason {
        FaultReason::Unmapped => 1,
        FaultReason::Permission => 2,
        FaultReason::AddressOverflow => 3,
    };
    context.set_word(48, fault.address.0);
    context.set_word(52, (width as u32 & 255) | access << 8 | reason << 16);
    context.set_word(116, (width as u32) << 8 | 14);
    StopReason::MemoryFault as u32
}

#[derive(Clone, Copy)]
pub(super) struct NumericBank {
    x87: X87State,
    vectors: VectorState,
    segments: SegmentContext,
}

impl NumericBank {
    pub(super) fn from_instance(instance: &EngineInstance) -> Result<Self, HostError> {
        instance.memory()?;
        let arena = instance.arena();
        Ok(Self {
            x87: decode_x87(&arena[X87_OFFSET..X87_OFFSET + X87_SIZE])
                .map_err(|_| HostError::Infrastructure)?,
            vectors: VectorState {
                simd: decode_simd(&arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE])
                    .map_err(|_| HostError::Infrastructure)?,
                upper: decode_ymm(&arena[YMM_OFFSET..YMM_OFFSET + YMM_SIZE])
                    .map_err(|_| HostError::Infrastructure)?
                    .registers,
            },
            segments: instance.segment_context()?,
        })
    }

    pub(super) fn publish_to_instance(
        &self,
        instance: &mut EngineInstance,
    ) -> Result<(), HostError> {
        instance.memory()?;
        let mut x87 = [0; X87_SIZE];
        let mut simd = [0; SIMD_SIZE];
        let mut ymm = [0; YMM_SIZE];
        encode_x87(&self.x87, &mut x87).map_err(|_| HostError::Infrastructure)?;
        encode_simd(&self.vectors.simd, &mut simd).map_err(|_| HostError::Infrastructure)?;
        encode_ymm(
            &YmmUpperState {
                registers: self.vectors.upper,
            },
            &mut ymm,
        )
        .map_err(|_| HostError::Infrastructure)?;
        let arena = instance.arena.as_mut().get_mut();
        arena[X87_OFFSET..X87_OFFSET + X87_SIZE].copy_from_slice(&x87);
        arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE].copy_from_slice(&simd);
        arena[YMM_OFFSET..YMM_OFFSET + YMM_SIZE].copy_from_slice(&ymm);
        Ok(())
    }

    pub(super) fn refresh_segments(&mut self, segments: SegmentContext) {
        self.segments = segments;
    }

    pub(super) fn architectural_state(
        &self,
    ) -> (
        X87State,
        crate::cpu::x86::simd::SimdState,
        YmmUpperState,
        SegmentContext,
    ) {
        (
            self.x87,
            self.vectors.simd,
            YmmUpperState {
                registers: self.vectors.upper,
            },
            self.segments,
        )
    }

    pub(super) fn execute(
        &mut self,
        instance: &mut EngineInstance,
        context: &mut Context32,
        instruction: &NativeInstruction,
    ) -> u32 {
        let Some(memory) = instance.memory.as_ref() else {
            return StopReason::Infrastructure as u32;
        };
        let before = memory.code_validation_version();
        let result = match instruction.operation {
            Operation::X87 {
                operation,
                address,
                waiting,
                opcode,
            } => self.x87(
                instance,
                context,
                instruction,
                operation,
                address,
                waiting,
                opcode,
            ),
            Operation::Simd(operation) => self.simd(instance, context, operation),
            Operation::Avx(operation) => self.avx(instance, context, operation),
            Operation::XState(operation) => self.xstate(instance, context, operation),
            Operation::StackFrame {
                kind,
                allocation,
                nesting,
            } => self.stack(instance, context, kind, allocation, nesting),
            Operation::InitializeX87 => self.x87(
                instance,
                context,
                instruction,
                X87Operation::Initialize,
                None,
                false,
                0,
            ),
            Operation::ClearX87Exceptions => self.x87(
                instance,
                context,
                instruction,
                X87Operation::ClearExceptions,
                None,
                false,
                0,
            ),
            Operation::X87StatusToAx => self.x87(
                instance,
                context,
                instruction,
                X87Operation::StatusToAx,
                None,
                false,
                0,
            ),
            Operation::X87StatusToMemory { address } => self.x87(
                instance,
                context,
                instruction,
                X87Operation::StoreStatus,
                Some(address),
                false,
                0,
            ),
            Operation::X87ControlToMemory { address } => self.x87(
                instance,
                context,
                instruction,
                X87Operation::StoreControl,
                Some(address),
                false,
                0,
            ),
            Operation::CpuId
            | Operation::ReadProcessorId { .. }
            | Operation::VirtualTimestamp
            | Operation::VirtualTimestampAux
            | Operation::SoftwareInterrupt { .. }
            | Operation::Control(crate::cpu::x86::ControlOperation::Sysenter) => {
                match self.processor(instance, context, instruction) {
                    Ok(true) => return StopReason::SystemEntry as u32,
                    Ok(false) => Ok(()),
                    Err(error) => Err(error),
                }
            }
            _ => return StopReason::Unsupported as u32,
        };
        match result {
            Err(error) => error.publish(context),
            Ok(())
                if instance
                    .memory
                    .as_ref()
                    .is_some_and(|m| m.code_validation_version() != before) =>
            {
                StopReason::CodeInvalidated as u32
            }
            Ok(()) => 0,
        }
    }
}

#[cfg(test)]
#[path = "numeric/tests.rs"]
mod tests;
