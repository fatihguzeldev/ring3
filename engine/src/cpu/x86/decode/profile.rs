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
    let x87_control = matches!(instruction.code(), Code::Fninit) && bytes == [0xdb, 0xe3]
        || matches!(instruction.code(), Code::Fnclex) && bytes == [0xdb, 0xe2]
        || matches!(instruction.code(), Code::Fnstsw_AX) && bytes == [0xdf, 0xe0]
        || matches!(
            instruction.code(),
            Code::Fnstsw_m2byte | Code::Fnstcw_m2byte
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
    let exact_word_string = instruction.code() == Code::Lodsw_AX_m16 && bytes == [0x66, 0xad]
        || instruction.code() == Code::Stosw_m16_AX && bytes == [0x66, 0xab]
        || instruction.code() == Code::Movsw_m16_m16 && bytes == [0x66, 0xa5]
        || instruction.code() == Code::Cmpsw_m16_m16 && bytes == [0x66, 0xa7]
        || instruction.code() == Code::Scasw_AX_m16 && bytes == [0x66, 0xaf];
    let exact_word_move = match (instruction.code(), bytes) {
        (Code::Mov_rm16_r16, [0x66, 0x89, modrm]) | (Code::Mov_r16_rm16, [0x66, 0x8b, modrm]) => {
            *modrm & 0xc0 == 0xc0
        }
        (Code::Mov_r16_imm16, [0x66, opcode, _, _]) => (0xb8..=0xbf).contains(opcode),
        (Code::Mov_rm16_imm16, [0x66, 0xc7, modrm, _, _]) => *modrm & 0xf8 == 0xc0,
        _ => false,
    };
    let exact_word_predicate = match (instruction.code(), bytes) {
        (Code::Cmp_rm16_r16, [0x66, 0x39, modrm])
        | (Code::Cmp_r16_rm16, [0x66, 0x3b, modrm])
        | (Code::Test_rm16_r16, [0x66, 0x85, modrm]) => *modrm & 0xc0 == 0xc0,
        (Code::Cmp_AX_imm16, [0x66, 0x3d, _, _]) | (Code::Test_AX_imm16, [0x66, 0xa9, _, _]) => {
            true
        }
        (Code::Cmp_rm16_imm16, [0x66, 0x81, modrm, _, _])
        | (Code::Cmp_rm16_imm8, [0x66, 0x83, modrm, _]) => *modrm & 0xf8 == 0xf8,
        (Code::Test_rm16_imm16, [0x66, 0xf7, modrm, _, _]) => *modrm & 0xf8 == 0xc0,
        _ => false,
    };
    let exact_word_arithmetic = match (instruction.code(), bytes) {
        (Code::Add_rm16_r16, [0x66, 0x01, modrm])
        | (Code::Add_r16_rm16, [0x66, 0x03, modrm])
        | (Code::Sub_rm16_r16, [0x66, 0x29, modrm])
        | (Code::Sub_r16_rm16, [0x66, 0x2b, modrm]) => *modrm & 0xc0 == 0xc0,
        (Code::Add_AX_imm16, [0x66, 0x05, _, _]) | (Code::Sub_AX_imm16, [0x66, 0x2d, _, _]) => true,
        (Code::Add_rm16_imm16, [0x66, 0x81, modrm, _, _])
        | (Code::Add_rm16_imm8, [0x66, 0x83, modrm, _]) => *modrm & 0xf8 == 0xc0,
        (Code::Sub_rm16_imm16, [0x66, 0x81, modrm, _, _])
        | (Code::Sub_rm16_imm8, [0x66, 0x83, modrm, _]) => *modrm & 0xf8 == 0xe8,
        _ => false,
    };
    if instruction.has_lock_prefix()
        || instruction.has_rep_prefix()
        || instruction.has_repne_prefix()
        || (!(exact_word_string
            || exact_word_move
            || exact_word_predicate
            || exact_word_arithmetic)
            && bytes
                .iter()
                .take_while(|byte| is_prefix(**byte))
                .any(|byte| matches!(*byte, 0x66 | 0x67)))
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
