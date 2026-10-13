use crate::{
    abi::{
        arena::{HELPER_OFFSET, SIMD_OFFSET, TRANSFER_OFFSET, YMM_OFFSET},
        cpu_helper::{CpuMemoryError, encode_cpu_result},
        memory_helper::HELPER_SIZE,
        x86::{
            AVX_ADDRESS_SIZE, AVX_ADDRESS_TRANSFER, AVX_OUTPUT_SIZE, AVX_OUTPUT_TRANSFER,
            SIMD_MXCSR_OFFSET, SIMD_SIZE, USER_FLAGS_MASK_V3, YMM_SIZE, decode_avx_address,
            decode_simd, decode_ymm, encode_simd, encode_ymm,
        },
    },
    cpu::x86::{
        SegmentAccess, SegmentContext, SegmentRegister, State32,
        avx::{
            AvxInstruction, AvxSemantic, MaskedError, MemoryEffect, ScalarInstruction, VectorState,
            VectorWidth,
            dispatch::{self, DispatchError},
        },
        simd::ymm::YmmUpperState,
    },
    memory::{Access, GuestAddress},
    process::{EngineInstance, HostError, StoreCompletion},
};

mod gather;

struct Publication {
    simd: [u8; SIMD_SIZE],
    upper: [u8; YMM_SIZE],
    output: [u8; AVX_OUTPUT_SIZE],
    packet: [u8; HELPER_SIZE],
}

impl Publication {
    fn prepare(
        instruction: AvxInstruction,
        vectors: &VectorState,
        general: &State32,
        length: u32,
    ) -> Result<Self, HostError> {
        let mut result = Self {
            simd: [0; SIMD_SIZE],
            upper: [0; YMM_SIZE],
            output: [0; AVX_OUTPUT_SIZE],
            packet: [0; HELPER_SIZE],
        };
        encode_simd(&vectors.simd, &mut result.simd).map_err(|_| HostError::Infrastructure)?;
        encode_ymm(
            &YmmUpperState {
                registers: vectors.upper,
            },
            &mut result.upper,
        )
        .map_err(|_| HostError::Infrastructure)?;
        let gpr = instruction
            .output()
            .map_or(0, |register| general.registers[register.index()]);
        result.output[..4].copy_from_slice(&gpr.to_le_bytes());
        result.output[4..8].copy_from_slice(&general.eflags.to_le_bytes());
        encode_cpu_result(length, 0, Ok(()), &mut result.packet)
            .map_err(|_| HostError::Infrastructure)?;
        Ok(result)
    }
    fn publish(self, arena: &mut [u8]) {
        arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE].copy_from_slice(&self.simd);
        arena[YMM_OFFSET..YMM_OFFSET + YMM_SIZE].copy_from_slice(&self.upper);
        arena[TRANSFER_OFFSET + AVX_OUTPUT_TRANSFER
            ..TRANSFER_OFFSET + AVX_OUTPUT_TRANSFER + AVX_OUTPUT_SIZE]
            .copy_from_slice(&self.output);
        arena[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE].copy_from_slice(&self.packet);
    }
}

impl EngineInstance {
    pub fn avx_execute(
        &mut self,
        descriptor: u32,
        operands: u32,
        immediate: u32,
        live0: u32,
        live1: u32,
        flags: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material(self.key, self.generation())?;
        self.avx_apply(None, descriptor, operands, immediate, [live0, live1], flags)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn avx_execute_resident(
        &mut self,
        key: u64,
        id: u64,
        descriptor: u32,
        operands: u32,
        immediate: u32,
        live0: u32,
        live1: u32,
        flags: u32,
    ) -> Result<StoreCompletion, HostError> {
        self.guard_material_resident(key, id)?;
        self.avx_apply(
            Some(id),
            descriptor,
            operands,
            immediate,
            [live0, live1],
            flags,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn avx_apply(
        &mut self,
        resident: Option<u64>,
        descriptor: u32,
        operands: u32,
        immediate: u32,
        live: [u32; 2],
        flags: u32,
    ) -> Result<StoreCompletion, HostError> {
        let instruction = AvxInstruction::decode_descriptor(descriptor, operands, immediate)
            .map_err(|_| HostError::InvalidRequest)?;
        if (matches!(instruction.semantic(), AvxSemantic::F16c(_))
            && !crate::cpu::x86::capabilities::F16C_ENABLED)
            || (matches!(instruction.semantic(), AvxSemantic::Fma(_))
                && !crate::cpu::x86::capabilities::FMA_ENABLED)
            || (matches!(instruction.semantic(), AvxSemantic::Integer(value)
                if value.width() == VectorWidth::Ymm)
                && !crate::cpu::x86::capabilities::AVX2_ENABLED)
            || (matches!(instruction.semantic(), AvxSemantic::Shift(value)
                if value.requires_avx2())
                && !crate::cpu::x86::capabilities::AVX2_ENABLED)
            || (matches!(instruction.semantic(), AvxSemantic::Rearrange(value) if value.width() == VectorWidth::Ymm)
                && !crate::cpu::x86::capabilities::AVX2_ENABLED)
            || (matches!(instruction.semantic(), AvxSemantic::CrossLane(value) if value.requires_avx2())
                && !crate::cpu::x86::capabilities::AVX2_ENABLED)
            || (matches!(instruction.semantic(), AvxSemantic::Multiply(value) if value.width() == VectorWidth::Ymm)
                && !crate::cpu::x86::capabilities::AVX2_ENABLED)
            || (matches!(instruction.semantic(), AvxSemantic::Permute(value) if value.requires_avx2())
                && !crate::cpu::x86::capabilities::AVX2_ENABLED)
            || (matches!(
                instruction.semantic(),
                AvxSemantic::Scalar(ScalarInstruction::Mask {
                    width: VectorWidth::Ymm,
                    ..
                })
            ) && !crate::cpu::x86::capabilities::AVX2_ENABLED)
            || (matches!(instruction.semantic(), AvxSemantic::Gather(_))
                && !crate::cpu::x86::capabilities::AVX2_ENABLED)
        {
            return Err(HostError::InvalidRequest);
        }
        if flags & 2 == 0
            || flags & !USER_FLAGS_MASK_V3 != 0
            || instruction
                .inputs()
                .into_iter()
                .zip(live)
                .any(|(register, value)| register.is_none() && value != 0)
        {
            return Err(HostError::InvalidRequest);
        }
        let arena = self.arena.as_ref().get_ref();
        let mut vectors = VectorState {
            simd: decode_simd(&arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE])
                .map_err(|_| HostError::Infrastructure)?,
            upper: decode_ymm(&arena[YMM_OFFSET..YMM_OFFSET + YMM_SIZE])
                .map_err(|_| HostError::Infrastructure)?
                .registers,
        };
        if let AvxSemantic::Gather(gather) = instruction.semantic() {
            return self.avx_gather_apply(gather, &vectors);
        }
        let mut general = State32 {
            eflags: flags,
            ..State32::default()
        };
        for (register, value) in instruction.inputs().into_iter().zip(live) {
            if let Some(register) = register {
                general.registers[register.index()] = value;
            }
        }
        let effect = instruction.memory();
        let length = match effect {
            MemoryEffect::None => 0,
            MemoryEffect::Full(requirement) => u32::from(requirement.bytes),
            MemoryEffect::Selected(masked) => masked.width().bytes() as u32,
            MemoryEffect::Gather(_) => unreachable!("gather uses its staged memory route"),
        };
        let address = if effect == MemoryEffect::None {
            None
        } else {
            let start = TRANSFER_OFFSET + AVX_ADDRESS_TRANSFER;
            Some(
                decode_avx_address(&arena[start..start + AVX_ADDRESS_SIZE])
                    .map_err(|_| HostError::InvalidRequest)?,
            )
        };
        if let MemoryEffect::Selected(masked) = effect {
            let address = address.ok_or(HostError::Infrastructure)?;
            let selected = masked.selected_bytes(vectors.read(masked.mask()));
            let context = if selected == 0 {
                SegmentContext::default()
            } else {
                self.segment_context()?
            };
            // masked stores change no CPU state; finish all encodes before the RAM transaction.
            let prepared = if masked.access() == Access::Write {
                Some(Publication::prepare(
                    instruction,
                    &vectors,
                    &general,
                    length,
                )?)
            } else {
                None
            };
            let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
            let version = memory.code_validation_version();
            let result = vectors.execute_masked(
                masked,
                &context,
                address.segment,
                if selected == 0 { 0 } else { address.offset },
                memory,
            );
            let changed = result.is_ok() && version != memory.code_validation_version();
            if let Err(error) = result {
                let error = match error {
                    MaskedError::InvalidState => return Err(HostError::Infrastructure),
                    MaskedError::Memory(error) => CpuMemoryError::Memory(error),
                    MaskedError::Segment { segment, .. } => segment_fault(segment),
                };
                return self.avx_fault(length, error);
            }
            let prepared = match prepared {
                Some(prepared) => prepared,
                None => Publication::prepare(instruction, &vectors, &general, length)?,
            };
            prepared.publish(self.arena.as_mut().get_mut());
            return self.cpu_store_currentness(changed, resident);
        }
        let mut payload = [0; 32];
        let mut linear = 0;
        if let MemoryEffect::Full(requirement) = effect {
            if requirement.bytes == 0
                || requirement.bytes > 32
                || !matches!(requirement.alignment, 1 | 16 | 32)
            {
                return Err(HostError::Infrastructure);
            }
            let address = address.ok_or(HostError::Infrastructure)?;
            let context = self.segment_context()?;
            let access = if requirement.access == Access::Read {
                SegmentAccess::Read
            } else {
                SegmentAccess::Write
            };
            if let Err(error) = super::segments::check_cached_span(
                &context,
                address.segment,
                address.offset,
                length,
                access,
            ) {
                return self.avx_fault(length, error);
            }
            linear = context.state.bases[address.segment.index()].wrapping_add(address.offset);
            if linear & (u32::from(requirement.alignment) - 1) != 0 {
                return self.avx_fault(length, CpuMemoryError::GeneralProtection(0));
            }
            // the fixed software environment checks numerical-store authority first.
            // this probe changes neither ram nor its version; the atomic write remains below.
            if requirement.access == Access::Write
                && matches!(instruction.semantic(), AvxSemantic::F16c(_))
            {
                let memory = self.memory.as_ref().ok_or(HostError::Closed)?;
                if let Err(error) =
                    memory.cpu_check_span(GuestAddress(linear), length as usize, Access::Write)
                {
                    return self.avx_fault(length, CpuMemoryError::Memory(error));
                }
            }
            if requirement.access == Access::Read {
                let memory = self.memory.as_ref().ok_or(HostError::Closed)?;
                if let Err(error) =
                    memory.cpu_read(GuestAddress(linear), &mut payload[..length as usize])
                {
                    return self.avx_fault(length, CpuMemoryError::Memory(error));
                }
            }
        }
        if let Err(error) = dispatch::execute(
            instruction,
            &mut vectors,
            &mut general,
            linear,
            &mut payload[..length as usize],
        ) {
            return match error {
                DispatchError::InvalidState => Err(HostError::Infrastructure),
                DispatchError::GeneralProtection => {
                    self.avx_fault(length, CpuMemoryError::GeneralProtection(0))
                }
                DispatchError::FloatingPoint(mxcsr) => {
                    let mut packet = [0; HELPER_SIZE];
                    encode_cpu_result(
                        length,
                        0,
                        Err(CpuMemoryError::SimdFloatingPoint(mxcsr)),
                        &mut packet,
                    )
                    .map_err(|_| HostError::Infrastructure)?;
                    let arena = self.arena.as_mut().get_mut();
                    arena[SIMD_OFFSET + SIMD_MXCSR_OFFSET..SIMD_OFFSET + SIMD_MXCSR_OFFSET + 4]
                        .copy_from_slice(&mxcsr.to_le_bytes());
                    arena[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE].copy_from_slice(&packet);
                    Ok(StoreCompletion::Complete)
                }
            };
        }
        let prepared = Publication::prepare(instruction, &vectors, &general, length)?;
        let mut changed = false;
        if matches!(effect, MemoryEffect::Full(requirement) if requirement.access == Access::Write)
        {
            let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
            let version = memory.code_validation_version();
            if let Err(error) = memory.cpu_write(GuestAddress(linear), &payload[..length as usize])
            {
                return self.avx_fault(length, CpuMemoryError::Memory(error));
            }
            changed = version != memory.code_validation_version();
        }
        prepared.publish(self.arena.as_mut().get_mut());
        self.cpu_store_currentness(changed, resident)
    }

    fn avx_fault(
        &mut self,
        length: u32,
        error: CpuMemoryError,
    ) -> Result<StoreCompletion, HostError> {
        let mut packet = [0; HELPER_SIZE];
        encode_cpu_result(length, 0, Err(error), &mut packet)
            .map_err(|_| HostError::Infrastructure)?;
        self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]
            .copy_from_slice(&packet);
        Ok(StoreCompletion::Complete)
    }
}

fn segment_fault(segment: SegmentRegister) -> CpuMemoryError {
    if segment == SegmentRegister::Ss {
        CpuMemoryError::StackSegment(0)
    } else {
        CpuMemoryError::GeneralProtection(0)
    }
}

#[cfg(test)]
#[path = "avx/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "avx/fma/native.rs"]
mod fma_tests;
