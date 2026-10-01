use iced_x86::{Code, CpuidFeature, Instruction, OpKind, Register};

use super::{DecodeError, operands::unsupported};
use crate::cpu::UnsupportedFeature;

pub(super) fn check_profile(instruction: &Instruction, bytes: &[u8]) -> Result<(), DecodeError> {
    let features = instruction.cpuid_features();
    let registers = (0..instruction.op_count())
        .filter(|&index| instruction.op_kind(index) == OpKind::Register)
        .map(|index| instruction.op_register(index));
    if instruction.is_privileged() {
        return Err(DecodeError::Unsupported(UnsupportedFeature::Privileged));
    }
    if features.contains(&CpuidFeature::FPU) || registers.clone().any(|register| register.is_st()) {
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
    if instruction.segment_prefix() != Register::None
        || registers
            .clone()
            .any(|register| register.is_segment_register())
    {
        return Err(DecodeError::Unsupported(UnsupportedFeature::Segment));
    }
    if instruction.is_string_instruction()
        && (instruction.has_rep_prefix() || instruction.has_repne_prefix())
    {
        return Err(DecodeError::Unsupported(UnsupportedFeature::RepeatedString));
    }
    if instruction.has_lock_prefix()
        || instruction.has_rep_prefix()
        || instruction.has_repne_prefix()
        || bytes
            .iter()
            .take_while(|byte| is_prefix(**byte))
            .any(|byte| matches!(*byte, 0x66 | 0x67))
    {
        return Err(unsupported());
    }
    Ok(())
}

fn is_prefix(byte: u8) -> bool {
    matches!(
        byte,
        0xf0 | 0xf2 | 0xf3 | 0x66 | 0x67 | 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65
    )
}
