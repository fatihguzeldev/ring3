use iced_x86::{Code, CpuidFeature, Instruction, OpKind};

use super::{DecodeError, operands::unsupported};
use crate::cpu::UnsupportedFeature;

pub(super) fn check_profile(instruction: &Instruction, bytes: &[u8]) -> Result<(), DecodeError> {
    if instruction.is_privileged() {
        return Err(DecodeError::Unsupported(UnsupportedFeature::Privileged));
    }
    let mut groups = [0_u8; 3];
    for &byte in bytes.iter().take_while(|byte| is_prefix(**byte)) {
        match byte {
            0x66 => groups[0] += 1,
            0xf0 => groups[1] += 1,
            0xf2 | 0xf3 => groups[2] += 1,
            _ => {}
        }
    }
    // duplicate prefixes in one group have unspecified behavior; keep refusal explicit.
    if groups.iter().any(|count| *count > 1) {
        return Err(unsupported());
    }
    if let Some(operation) = super::control::lower(instruction) {
        return operation.map(|_| ());
    }
    if let Some(operation) = super::stack::lower(instruction) {
        return operation.map(|_| ());
    }
    if let Some(operation) = super::strings::lower(instruction, bytes) {
        return operation.map(|_| ());
    }
    if let Some(operation) = super::segments::lower(instruction) {
        return operation.map(|_| ());
    }
    if super::x87::decode(instruction).is_some() {
        return Ok(());
    }
    if let Some(operation) = super::simd::lower(instruction) {
        return operation.map(|_| ());
    }
    if let Some(operation) = super::atomic::lower(instruction) {
        return operation.map(|_| ());
    }
    if matches!(
        instruction.code(),
        Code::Movbe_r16_m16 | Code::Movbe_r32_m32 | Code::Movbe_m16_r16 | Code::Movbe_m32_r32
    ) && groups[2] != 0
    {
        return Err(unsupported());
    }
    if let Some(operation) = super::scalar_bits::lower(instruction) {
        return operation.map(|_| ());
    }
    if let Some(operation) = super::bit_manipulation::lower(instruction) {
        return operation.map(|_| ());
    }
    let features = instruction.cpuid_features();
    let registers = (0..instruction.op_count())
        .filter(|&index| instruction.op_kind(index) == OpKind::Register)
        .map(|index| instruction.op_register(index));
    let x87_control = matches!(
        instruction.code(),
        Code::Fninit | Code::Fnclex | Code::Fnstsw_AX | Code::Fnstsw_m2byte | Code::Fnstcw_m2byte
    );
    if !x87_control
        && (features.contains(&CpuidFeature::FPU)
            || registers.clone().any(|register| register.is_st()))
    {
        return Err(DecodeError::Unsupported(UnsupportedFeature::FloatingPoint));
    }
    if instruction.code() != Code::Pause
        && (registers.clone().any(|register| {
            register.is_xmm() || register.is_ymm() || register.is_zmm() || register.is_mm()
        }) || features.iter().any(|feature| {
            matches!(
                feature,
                CpuidFeature::MMX
                    | CpuidFeature::SSE
                    | CpuidFeature::SSE2
                    | CpuidFeature::SSE3
                    | CpuidFeature::SSSE3
                    | CpuidFeature::SSE4_1
                    | CpuidFeature::SSE4_2
                    | CpuidFeature::AVX
                    | CpuidFeature::AVX2
                    | CpuidFeature::AVX512F
            )
        }))
    {
        return Err(DecodeError::Unsupported(UnsupportedFeature::Simd));
    }
    if registers
        .clone()
        .any(|register| register.is_segment_register())
    {
        return Err(DecodeError::Unsupported(UnsupportedFeature::Segment));
    }
    let locked_scalar = matches!(
        instruction.code(),
        Code::Xchg_rm8_r8
            | Code::Xchg_rm16_r16
            | Code::Xchg_rm32_r32
            | Code::Cmpxchg_rm8_r8
            | Code::Cmpxchg_rm16_r16
            | Code::Cmpxchg_rm32_r32
            | Code::Xadd_rm8_r8
            | Code::Xadd_rm16_r16
            | Code::Xadd_rm32_r32
            | Code::Bts_rm32_r32
            | Code::Btr_rm32_r32
            | Code::Or_rm32_imm8
            | Code::Or_rm32_imm32
            | Code::Or_rm32_r32
            | Code::And_rm32_imm8
            | Code::And_rm32_imm32
            | Code::And_rm32_r32
            | Code::Xor_rm32_imm8
            | Code::Xor_rm32_imm32
            | Code::Xor_rm32_r32
    );
    if instruction.has_lock_prefix() && (!locked_scalar || instruction.op0_kind() != OpKind::Memory)
    {
        return Err(unsupported());
    }
    let mandatory_repeat = matches!(instruction.code(), Code::Tzcnt_r32_rm32 | Code::Pause);
    if instruction.has_repne_prefix() || instruction.has_rep_prefix() && !mandatory_repeat {
        return Err(unsupported());
    }
    // decoded operand width owns 66 semantics, including byte operations that ignore it.
    Ok(())
}

fn is_prefix(byte: u8) -> bool {
    matches!(
        byte,
        0xf0 | 0xf2 | 0xf3 | 0x66 | 0x67 | 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65
    )
}
