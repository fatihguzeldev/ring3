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
    let exact_cs_alignment_nop = instruction.code() == Code::Nop_rm16
        && bytes == [0x66, 0x2e, 0x0f, 0x1f, 0x84, 0, 0, 0, 0, 0];
    let exact_alignment_nop = exact_cs_alignment_nop
        || instruction.code() == Code::Nop_rm16
            && matches!(
                bytes,
                [0x66, 0x0f, 0x1f, 0x44, 0, 0] | [0x66, 0x0f, 0x1f, 0x84, 0, 0, 0, 0, 0]
            );
    if (instruction.segment_prefix() != Register::None && !exact_cs_alignment_nop)
        || registers
            .clone()
            .any(|register| register.is_segment_register())
    {
        return Err(DecodeError::Unsupported(UnsupportedFeature::Segment));
    }
    if instruction.code() == Code::Pause && bytes != [0xf3, 0x90] {
        return Err(unsupported());
    }
    if matches!(instruction.code(), Code::Daa | Code::Das)
        && !matches!(
            (instruction.code(), bytes),
            (Code::Daa, [0x27]) | (Code::Das, [0x2f])
        )
    {
        return Err(unsupported());
    }
    if matches!(instruction.code(), Code::Aaa | Code::Aas)
        && !matches!(
            (instruction.code(), bytes),
            (Code::Aaa, [0x37]) | (Code::Aas, [0x3f])
        )
    {
        return Err(unsupported());
    }
    if instruction.code() == Code::Aad_imm8 && !matches!(bytes, [0xd5, _]) {
        return Err(unsupported());
    }
    if instruction.code() == Code::Aam_imm8 && !matches!(bytes, [0xd4, _]) {
        return Err(unsupported());
    }
    if instruction.code() == Code::Xchg_rm32_r32
        && instruction.op0_kind() == OpKind::Memory
        && !matches!(bytes, [0x87, modrm, ..] if *modrm & 0xc0 != 0xc0)
    {
        return Err(unsupported());
    }
    if instruction.code() == Code::Xchg_rm8_r8
        && instruction.op0_kind() == OpKind::Memory
        && !matches!(bytes, [0x86, modrm, ..] if *modrm & 0xc0 != 0xc0)
    {
        return Err(unsupported());
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
    let exact_word_accumulator_extend = matches!(
        (instruction.code(), bytes),
        (Code::Cbw, [0x66, 0x98]) | (Code::Cwd, [0x66, 0x99])
    );
    let exact_word_exchange = match (instruction.code(), bytes) {
        (Code::Xchg_rm16_r16, [0x66, 0x87, modrm]) => *modrm & 0xc0 == 0xc0,
        (Code::Xchg_r16_AX, [0x66, 0x91..=0x97]) | (Code::Nopw, [0x66, 0x90]) => true,
        _ => false,
    };
    let exact_word_byte_extend = matches!(
        (instruction.code(), bytes),
        (Code::Movzx_r16_rm8, [0x66, 0x0f, 0xb6, _, ..])
            | (Code::Movsx_r16_rm8, [0x66, 0x0f, 0xbe, _, ..])
    );
    let exact_word_lea = matches!(
        (instruction.code(), bytes),
        (Code::Lea_r16_m, [0x66, 0x8d, modrm, ..]) if *modrm & 0xc0 != 0xc0
    );
    let exact_word_near_control = matches!(
        (instruction.code(), bytes),
        (Code::Call_rel16, [0x66, 0xe8, _, _])
            | (Code::Jmp_rel16, [0x66, 0xe9, _, _])
            | (Code::Retnw, [0x66, 0xc3])
            | (Code::Retnw_imm16, [0x66, 0xc2, _, _])
            | (
                Code::Jo_rel16
                    | Code::Jno_rel16
                    | Code::Jb_rel16
                    | Code::Jae_rel16
                    | Code::Je_rel16
                    | Code::Jne_rel16
                    | Code::Jbe_rel16
                    | Code::Ja_rel16
                    | Code::Js_rel16
                    | Code::Jns_rel16
                    | Code::Jp_rel16
                    | Code::Jnp_rel16
                    | Code::Jl_rel16
                    | Code::Jge_rel16
                    | Code::Jle_rel16
                    | Code::Jg_rel16,
                [0x66, 0x0f, 0x80..=0x8f, _, _],
            )
    );
    let exact_word_indirect_jump = matches!(
        (instruction.code(), bytes),
        (Code::Jmp_rm16, [0x66, 0xff, modrm, ..]) if *modrm & 0x38 == 0x20
    );
    let exact_word_indirect_call = matches!(
        (instruction.code(), bytes),
        (Code::Call_rm16, [0x66, 0xff, modrm, ..]) if *modrm & 0x38 == 0x10
    );
    let exact_word_stack = match (instruction.code(), bytes) {
        (Code::Push_r16, [0x66, 0x50..=0x57])
        | (Code::Pop_r16, [0x66, 0x58..=0x5f])
        | (Code::Popaw, [0x66, 0x61])
        | (Code::Push_imm16, [0x66, 0x68, _, _])
        | (Code::Pushw_imm8, [0x66, 0x6a, _])
        | (Code::Pushfw, [0x66, 0x9c])
        | (Code::Leavew, [0x66, 0xc9]) => true,
        (Code::Push_rm16, [0x66, 0xff, modrm, ..]) => *modrm & 0x38 == 0x30,
        (Code::Pop_rm16, [0x66, 0x8f, modrm, ..]) => *modrm & 0x38 == 0,
        _ => false,
    };
    let exact_enter_zero = matches!(
        (instruction.code(), bytes),
        (Code::Enterd_imm16_imm8, [0xc8, 0, 0, 0])
            | (Code::Enterw_imm16_imm8, [0x66, 0xc8, 0, 0, 0])
    );
    if matches!(
        instruction.code(),
        Code::Enterd_imm16_imm8 | Code::Enterw_imm16_imm8
    ) && !exact_enter_zero
    {
        return Err(unsupported());
    }
    let exact_word_move = match (instruction.code(), bytes) {
        (Code::Mov_AX_moffs16, [0x66, 0xa1, _, _, _, _])
        | (Code::Mov_moffs16_AX, [0x66, 0xa3, _, _, _, _]) => true,
        (Code::Mov_rm16_r16, [0x66, 0x89, modrm, ..])
        | (Code::Mov_r16_rm16, [0x66, 0x8b, modrm, ..])
            if *modrm & 0xc0 != 0xc0 =>
        {
            true
        }
        (Code::Mov_rm16_imm16, [0x66, 0xc7, modrm, ..])
            if *modrm & 0xc0 != 0xc0 && *modrm & 0x38 == 0 =>
        {
            true
        }
        (Code::Mov_rm16_r16, [0x66, 0x89, modrm]) | (Code::Mov_r16_rm16, [0x66, 0x8b, modrm]) => {
            *modrm & 0xc0 == 0xc0
        }
        (Code::Mov_r16_imm16, [0x66, opcode, _, _]) => (0xb8..=0xbf).contains(opcode),
        (Code::Mov_rm16_imm16, [0x66, 0xc7, modrm, _, _]) => *modrm & 0xf8 == 0xc0,
        _ => false,
    };
    let exact_word_predicate = match (instruction.code(), bytes) {
        (Code::Cmp_rm16_r16, [0x66, 0x39, _, ..])
        | (Code::Cmp_r16_rm16, [0x66, 0x3b, _, ..])
        | (Code::Test_rm16_r16, [0x66, 0x85, _, ..]) => true,
        (Code::Cmp_AX_imm16, [0x66, 0x3d, _, _]) | (Code::Test_AX_imm16, [0x66, 0xa9, _, _]) => {
            true
        }
        (Code::Cmp_rm16_imm16, [0x66, 0x81, modrm, ..])
        | (Code::Cmp_rm16_imm8, [0x66, 0x83, modrm, ..]) => *modrm & 0x38 == 0x38,
        (Code::Test_rm16_imm16, [0x66, 0xf7, modrm, ..]) => *modrm & 0x38 == 0,
        _ => false,
    };
    let exact_word_arithmetic = match (instruction.code(), bytes) {
        (Code::Add_rm16_r16, [0x66, 0x01, modrm, ..])
        | (Code::Sub_rm16_r16, [0x66, 0x29, modrm, ..])
        | (Code::Adc_rm16_r16, [0x66, 0x11, modrm, ..])
        | (Code::Sbb_rm16_r16, [0x66, 0x19, modrm, ..])
            if *modrm & 0xc0 != 0xc0 =>
        {
            true
        }
        (Code::Add_r16_rm16, [0x66, 0x03, modrm, ..])
        | (Code::Sub_r16_rm16, [0x66, 0x2b, modrm, ..])
        | (Code::Adc_r16_rm16, [0x66, 0x13, modrm, ..])
        | (Code::Sbb_r16_rm16, [0x66, 0x1b, modrm, ..])
            if *modrm & 0xc0 != 0xc0 =>
        {
            true
        }
        (Code::Add_rm16_r16, [0x66, 0x01, modrm])
        | (Code::Add_r16_rm16, [0x66, 0x03, modrm])
        | (Code::Sub_rm16_r16, [0x66, 0x29, modrm])
        | (Code::Sub_r16_rm16, [0x66, 0x2b, modrm])
        | (Code::Adc_rm16_r16, [0x66, 0x11, modrm])
        | (Code::Adc_r16_rm16, [0x66, 0x13, modrm])
        | (Code::Sbb_rm16_r16, [0x66, 0x19, modrm])
        | (Code::Sbb_r16_rm16, [0x66, 0x1b, modrm]) => *modrm & 0xc0 == 0xc0,
        (Code::Add_AX_imm16, [0x66, 0x05, _, _])
        | (Code::Sub_AX_imm16, [0x66, 0x2d, _, _])
        | (Code::Adc_AX_imm16, [0x66, 0x15, _, _])
        | (Code::Sbb_AX_imm16, [0x66, 0x1d, _, _]) => true,
        (Code::Add_rm16_imm16, [0x66, 0x81, modrm, ..])
        | (Code::Add_rm16_imm8, [0x66, 0x83, modrm, ..])
            if *modrm & 0xc0 != 0xc0 =>
        {
            *modrm & 0x38 == 0
        }
        (Code::Sub_rm16_imm16, [0x66, 0x81, modrm, ..])
        | (Code::Sub_rm16_imm8, [0x66, 0x83, modrm, ..])
            if *modrm & 0xc0 != 0xc0 =>
        {
            *modrm & 0x38 == 0x28
        }
        (Code::Adc_rm16_imm16, [0x66, 0x81, modrm, ..])
        | (Code::Adc_rm16_imm8, [0x66, 0x83, modrm, ..])
            if *modrm & 0xc0 != 0xc0 =>
        {
            *modrm & 0x38 == 0x10
        }
        (Code::Sbb_rm16_imm16, [0x66, 0x81, modrm, ..])
        | (Code::Sbb_rm16_imm8, [0x66, 0x83, modrm, ..])
            if *modrm & 0xc0 != 0xc0 =>
        {
            *modrm & 0x38 == 0x18
        }
        (Code::Add_rm16_imm16, [0x66, 0x81, modrm, _, _])
        | (Code::Add_rm16_imm8, [0x66, 0x83, modrm, _]) => *modrm & 0xf8 == 0xc0,
        (Code::Sub_rm16_imm16, [0x66, 0x81, modrm, _, _])
        | (Code::Sub_rm16_imm8, [0x66, 0x83, modrm, _]) => *modrm & 0xf8 == 0xe8,
        (Code::Adc_rm16_imm16, [0x66, 0x81, modrm, _, _])
        | (Code::Adc_rm16_imm8, [0x66, 0x83, modrm, _]) => *modrm & 0xf8 == 0xd0,
        (Code::Sbb_rm16_imm16, [0x66, 0x81, modrm, _, _])
        | (Code::Sbb_rm16_imm8, [0x66, 0x83, modrm, _]) => *modrm & 0xf8 == 0xd8,
        _ => false,
    };
    let exact_word_logical = match (instruction.code(), bytes) {
        (Code::And_rm16_r16, [0x66, 0x21, modrm, ..])
        | (Code::Or_rm16_r16, [0x66, 0x09, modrm, ..])
        | (Code::Xor_rm16_r16, [0x66, 0x31, modrm, ..])
            if *modrm & 0xc0 != 0xc0 =>
        {
            true
        }
        (Code::And_r16_rm16, [0x66, 0x23, modrm, ..])
        | (Code::Or_r16_rm16, [0x66, 0x0b, modrm, ..])
        | (Code::Xor_r16_rm16, [0x66, 0x33, modrm, ..])
            if *modrm & 0xc0 != 0xc0 =>
        {
            true
        }
        (Code::And_rm16_r16, [0x66, 0x21, modrm])
        | (Code::And_r16_rm16, [0x66, 0x23, modrm])
        | (Code::Or_rm16_r16, [0x66, 0x09, modrm])
        | (Code::Or_r16_rm16, [0x66, 0x0b, modrm])
        | (Code::Xor_rm16_r16, [0x66, 0x31, modrm])
        | (Code::Xor_r16_rm16, [0x66, 0x33, modrm]) => *modrm & 0xc0 == 0xc0,
        (Code::And_AX_imm16, [0x66, 0x25, _, _])
        | (Code::Or_AX_imm16, [0x66, 0x0d, _, _])
        | (Code::Xor_AX_imm16, [0x66, 0x35, _, _]) => true,
        (Code::And_rm16_imm16, [0x66, 0x81, modrm, ..])
        | (Code::And_rm16_imm8, [0x66, 0x83, modrm, ..])
            if *modrm & 0xc0 != 0xc0 =>
        {
            *modrm & 0x38 == 0x20
        }
        (Code::Or_rm16_imm16, [0x66, 0x81, modrm, ..])
        | (Code::Or_rm16_imm8, [0x66, 0x83, modrm, ..])
            if *modrm & 0xc0 != 0xc0 =>
        {
            *modrm & 0x38 == 0x08
        }
        (Code::Xor_rm16_imm16, [0x66, 0x81, modrm, ..])
        | (Code::Xor_rm16_imm8, [0x66, 0x83, modrm, ..])
            if *modrm & 0xc0 != 0xc0 =>
        {
            *modrm & 0x38 == 0x30
        }
        (Code::And_rm16_imm16, [0x66, 0x81, modrm, _, _])
        | (Code::And_rm16_imm8, [0x66, 0x83, modrm, _]) => *modrm & 0xf8 == 0xe0,
        (Code::Or_rm16_imm16, [0x66, 0x81, modrm, _, _])
        | (Code::Or_rm16_imm8, [0x66, 0x83, modrm, _]) => *modrm & 0xf8 == 0xc8,
        (Code::Xor_rm16_imm16, [0x66, 0x81, modrm, _, _])
        | (Code::Xor_rm16_imm8, [0x66, 0x83, modrm, _]) => *modrm & 0xf8 == 0xf0,
        _ => false,
    };
    let exact_word_unary = match (instruction.code(), bytes) {
        (Code::Inc_r16, [0x66, opcode]) => (0x40..=0x47).contains(opcode),
        (Code::Dec_r16, [0x66, opcode]) => (0x48..=0x4f).contains(opcode),
        (Code::Inc_rm16, [0x66, 0xff, modrm, ..]) if *modrm & 0xc0 != 0xc0 => *modrm & 0x38 == 0,
        (Code::Dec_rm16, [0x66, 0xff, modrm, ..]) if *modrm & 0xc0 != 0xc0 => *modrm & 0x38 == 0x08,
        (Code::Not_rm16, [0x66, 0xf7, modrm, ..]) if *modrm & 0xc0 != 0xc0 => *modrm & 0x38 == 0x10,
        (Code::Neg_rm16, [0x66, 0xf7, modrm, ..]) if *modrm & 0xc0 != 0xc0 => *modrm & 0x38 == 0x18,
        (Code::Inc_rm16, [0x66, 0xff, modrm]) => *modrm & 0xf8 == 0xc0,
        (Code::Dec_rm16, [0x66, 0xff, modrm]) => *modrm & 0xf8 == 0xc8,
        (Code::Not_rm16, [0x66, 0xf7, modrm]) => *modrm & 0xf8 == 0xd0,
        (Code::Neg_rm16, [0x66, 0xf7, modrm]) => *modrm & 0xf8 == 0xd8,
        _ => false,
    };
    let exact_word_shift = match (instruction.code(), bytes) {
        (Code::Shl_rm16_1, [0x66, 0xd1, modrm, ..])
        | (Code::Shl_rm16_CL, [0x66, 0xd3, modrm, ..])
        | (Code::Shl_rm16_imm8, [0x66, 0xc1, modrm, ..])
            if *modrm & 0xc0 != 0xc0 =>
        {
            *modrm & 0x38 == 0x20
        }
        (Code::Shr_rm16_1, [0x66, 0xd1, modrm, ..])
        | (Code::Shr_rm16_CL, [0x66, 0xd3, modrm, ..])
        | (Code::Shr_rm16_imm8, [0x66, 0xc1, modrm, ..])
            if *modrm & 0xc0 != 0xc0 =>
        {
            *modrm & 0x38 == 0x28
        }
        (Code::Sar_rm16_1, [0x66, 0xd1, modrm, ..])
        | (Code::Sar_rm16_CL, [0x66, 0xd3, modrm, ..])
        | (Code::Sar_rm16_imm8, [0x66, 0xc1, modrm, ..])
            if *modrm & 0xc0 != 0xc0 =>
        {
            *modrm & 0x38 == 0x38
        }
        (Code::Shl_rm16_1, [0x66, 0xd1, modrm])
        | (Code::Shl_rm16_CL, [0x66, 0xd3, modrm])
        | (Code::Shl_rm16_imm8, [0x66, 0xc1, modrm, _]) => *modrm & 0xf8 == 0xe0,
        (Code::Shr_rm16_1, [0x66, 0xd1, modrm])
        | (Code::Shr_rm16_CL, [0x66, 0xd3, modrm])
        | (Code::Shr_rm16_imm8, [0x66, 0xc1, modrm, _]) => *modrm & 0xf8 == 0xe8,
        (Code::Sar_rm16_1, [0x66, 0xd1, modrm])
        | (Code::Sar_rm16_CL, [0x66, 0xd3, modrm])
        | (Code::Sar_rm16_imm8, [0x66, 0xc1, modrm, _]) => *modrm & 0xf8 == 0xf8,
        _ => false,
    };
    let exact_word_double_shift = match (instruction.code(), bytes) {
        (Code::Shld_rm16_r16_imm8, [0x66, 0x0f, 0xa4, modrm, ..])
        | (Code::Shrd_rm16_r16_imm8, [0x66, 0x0f, 0xac, modrm, ..])
        | (Code::Shld_rm16_r16_CL, [0x66, 0x0f, 0xa5, modrm, ..])
        | (Code::Shrd_rm16_r16_CL, [0x66, 0x0f, 0xad, modrm, ..])
            if *modrm & 0xc0 != 0xc0 =>
        {
            true
        }
        (Code::Shld_rm16_r16_imm8, [0x66, 0x0f, 0xa4, modrm, _])
        | (Code::Shrd_rm16_r16_imm8, [0x66, 0x0f, 0xac, modrm, _])
        | (Code::Shld_rm16_r16_CL, [0x66, 0x0f, 0xa5, modrm])
        | (Code::Shrd_rm16_r16_CL, [0x66, 0x0f, 0xad, modrm]) => *modrm & 0xc0 == 0xc0,
        _ => false,
    };
    let exact_word_rotate = match (instruction.code(), bytes) {
        (Code::Rol_rm16_1, [0x66, 0xd1, modrm, ..])
        | (Code::Rol_rm16_CL, [0x66, 0xd3, modrm, ..])
        | (Code::Rol_rm16_imm8, [0x66, 0xc1, modrm, ..])
            if *modrm & 0xc0 != 0xc0 =>
        {
            *modrm & 0x38 == 0
        }
        (Code::Ror_rm16_1, [0x66, 0xd1, modrm, ..])
        | (Code::Ror_rm16_CL, [0x66, 0xd3, modrm, ..])
        | (Code::Ror_rm16_imm8, [0x66, 0xc1, modrm, ..])
            if *modrm & 0xc0 != 0xc0 =>
        {
            *modrm & 0x38 == 0x08
        }
        (Code::Rol_rm16_1, [0x66, 0xd1, modrm])
        | (Code::Rol_rm16_CL, [0x66, 0xd3, modrm])
        | (Code::Rol_rm16_imm8, [0x66, 0xc1, modrm, _]) => *modrm & 0xf8 == 0xc0,
        (Code::Ror_rm16_1, [0x66, 0xd1, modrm])
        | (Code::Ror_rm16_CL, [0x66, 0xd3, modrm])
        | (Code::Ror_rm16_imm8, [0x66, 0xc1, modrm, _]) => *modrm & 0xf8 == 0xc8,
        (Code::Rcl_rm16_1, [0x66, 0xd1, modrm, ..])
        | (Code::Rcl_rm16_CL, [0x66, 0xd3, modrm, ..])
        | (Code::Rcl_rm16_imm8, [0x66, 0xc1, modrm, ..])
            if *modrm & 0xc0 != 0xc0 =>
        {
            *modrm & 0x38 == 0x10
        }
        (Code::Rcr_rm16_1, [0x66, 0xd1, modrm, ..])
        | (Code::Rcr_rm16_CL, [0x66, 0xd3, modrm, ..])
        | (Code::Rcr_rm16_imm8, [0x66, 0xc1, modrm, ..])
            if *modrm & 0xc0 != 0xc0 =>
        {
            *modrm & 0x38 == 0x18
        }
        (Code::Rcl_rm16_1, [0x66, 0xd1, modrm])
        | (Code::Rcl_rm16_CL, [0x66, 0xd3, modrm])
        | (Code::Rcl_rm16_imm8, [0x66, 0xc1, modrm, _]) => *modrm & 0xf8 == 0xd0,
        (Code::Rcr_rm16_1, [0x66, 0xd1, modrm])
        | (Code::Rcr_rm16_CL, [0x66, 0xd3, modrm])
        | (Code::Rcr_rm16_imm8, [0x66, 0xc1, modrm, _]) => *modrm & 0xf8 == 0xd8,
        _ => false,
    };
    let exact_word_xadd = matches!(
        (instruction.code(), bytes),
        (Code::Xadd_rm16_r16, [0x66, 0x0f, 0xc1, modrm]) if *modrm & 0xc0 == 0xc0
    ) || matches!(
        (instruction.code(), bytes),
        (Code::Xadd_rm16_r16, [0x66, 0x0f, 0xc1, modrm, ..]) if *modrm & 0xc0 != 0xc0
    );
    let exact_word_cmpxchg = matches!(
        (instruction.code(), bytes),
        (Code::Cmpxchg_rm16_r16, [0x66, 0x0f, 0xb1, modrm]) if *modrm & 0xc0 == 0xc0
    ) || matches!(
        (instruction.code(), bytes),
        (Code::Cmpxchg_rm16_r16, [0x66, 0x0f, 0xb1, modrm, ..]) if *modrm & 0xc0 != 0xc0
    );
    let exact_word_bit_scan = matches!(
        (instruction.code(), bytes),
        (Code::Bsf_r16_rm16, [0x66, 0x0f, 0xbc, _, ..])
            | (Code::Bsr_r16_rm16, [0x66, 0x0f, 0xbd, _, ..])
    );
    let exact_word_bit_test = match (instruction.code(), bytes) {
        (Code::Bt_rm16_r16, [0x66, 0x0f, 0xa3, modrm])
        | (Code::Bts_rm16_r16, [0x66, 0x0f, 0xab, modrm])
        | (Code::Btr_rm16_r16, [0x66, 0x0f, 0xb3, modrm])
        | (Code::Btc_rm16_r16, [0x66, 0x0f, 0xbb, modrm])
        | (
            Code::Bt_rm16_imm8 | Code::Bts_rm16_imm8 | Code::Btr_rm16_imm8 | Code::Btc_rm16_imm8,
            [0x66, 0x0f, 0xba, modrm, _],
        ) => *modrm & 0xc0 == 0xc0,
        _ => false,
    };
    let exact_word_memory_bit_test = match (instruction.code(), bytes) {
        (Code::Bt_rm16_r16, [0x66, 0x0f, 0xa3, modrm, ..])
        | (Code::Bts_rm16_r16, [0x66, 0x0f, 0xab, modrm, ..])
        | (Code::Btr_rm16_r16, [0x66, 0x0f, 0xb3, modrm, ..])
        | (Code::Btc_rm16_r16, [0x66, 0x0f, 0xbb, modrm, ..])
        | (
            Code::Bt_rm16_imm8 | Code::Bts_rm16_imm8 | Code::Btr_rm16_imm8 | Code::Btc_rm16_imm8,
            [0x66, 0x0f, 0xba, modrm, ..],
        ) => *modrm & 0xc0 != 0xc0,
        _ => false,
    };
    let exact_word_signed_multiply = matches!(
        (instruction.code(), bytes),
        (Code::Imul_r16_rm16, [0x66, 0x0f, 0xaf, _, ..])
            | (Code::Imul_r16_rm16_imm16, [0x66, 0x69, _, ..])
            | (Code::Imul_r16_rm16_imm8, [0x66, 0x6b, _, ..])
    );
    let exact_word_accumulator_multiply = matches!(
        (instruction.code(), bytes),
        (Code::Mul_rm16 | Code::Imul_rm16, [0x66, 0xf7, _, ..])
    );
    let exact_word_accumulator_divide = matches!(
        (instruction.code(), bytes),
        (Code::Div_rm16 | Code::Idiv_rm16, [0x66, 0xf7, _, ..])
    );
    let exact_word_conditional_move = matches!(
        (instruction.code(), bytes),
        (
            Code::Cmovo_r16_rm16
                | Code::Cmovno_r16_rm16
                | Code::Cmovb_r16_rm16
                | Code::Cmovae_r16_rm16
                | Code::Cmove_r16_rm16
                | Code::Cmovne_r16_rm16
                | Code::Cmovbe_r16_rm16
                | Code::Cmova_r16_rm16
                | Code::Cmovs_r16_rm16
                | Code::Cmovns_r16_rm16
                | Code::Cmovp_r16_rm16
                | Code::Cmovnp_r16_rm16
                | Code::Cmovl_r16_rm16
                | Code::Cmovge_r16_rm16
                | Code::Cmovle_r16_rm16
                | Code::Cmovg_r16_rm16,
            [0x66, 0x0f, 0x40..=0x4f, _, ..],
        )
    );
    if instruction.has_lock_prefix()
        || instruction.has_rep_prefix()
        || instruction.has_repne_prefix()
        || (!(exact_alignment_nop
            || exact_word_string
            || exact_word_accumulator_extend
            || exact_word_exchange
            || exact_word_byte_extend
            || exact_word_lea
            || exact_word_near_control
            || exact_word_indirect_jump
            || exact_word_indirect_call
            || exact_word_stack
            || exact_enter_zero
            || exact_word_move
            || exact_word_predicate
            || exact_word_arithmetic
            || exact_word_logical
            || exact_word_unary
            || exact_word_shift
            || exact_word_double_shift
            || exact_word_xadd
            || exact_word_cmpxchg
            || exact_word_bit_scan
            || exact_word_bit_test
            || exact_word_memory_bit_test
            || exact_word_signed_multiply
            || exact_word_accumulator_multiply
            || exact_word_accumulator_divide
            || exact_word_rotate
            || exact_word_conditional_move)
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
