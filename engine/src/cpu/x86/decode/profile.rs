use iced_x86::{Code, CpuidFeature, Instruction, OpKind};

use super::{DecodeError, operands::unsupported};
use crate::cpu::UnsupportedFeature;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PrefixAdmission {
    Nop,
    Instruction,
}

pub(super) fn check_prefixes(
    instruction: &Instruction,
    bytes: &[u8],
) -> Result<PrefixAdmission, DecodeError> {
    if instruction.is_privileged() && !super::system_registers::is_query(instruction.code()) {
        return Err(DecodeError::Unsupported(UnsupportedFeature::Privileged));
    }
    let mut groups = [0_u8; 3];
    let mut repeat_kinds = 0;
    for &byte in bytes.iter().take_while(|byte| is_prefix(**byte)) {
        match byte {
            0x66 => groups[0] += 1,
            0xf0 => groups[1] += 1,
            0xf2 | 0xf3 => {
                groups[2] += 1;
                repeat_kinds |= if byte == 0xf2 { 1 } else { 2 };
            }
            _ => {}
        }
    }
    let hle_hint = instruction.has_xacquire_prefix() || instruction.has_xrelease_prefix();
    // mixed hle hints select the closest repeat prefix; other duplicates stay explicit.
    let mixed_hle = !crate::cpu::x86::capabilities::HLE_ENABLED && repeat_kinds == 3 && hle_hint;
    // compiler alignment extends real multi-byte nops with repeated operand-size prefixes.
    let alignment_nop = matches!(instruction.code(), Code::Nop_rm16 | Code::Nop_rm32)
        && groups[1] == 0
        && groups[2] == 0;
    if groups[0] > 1 && !alignment_nop || groups[1] > 1 || groups[2] > 1 && !mixed_hle {
        return Err(unsupported());
    }
    if legacy_nop_alias(instruction) {
        if groups[1] != 0 || groups[2] != 0 {
            return Err(unsupported());
        }
        return Ok(PrefixAdmission::Nop);
    }
    if super::mpx::disabled_nop(instruction) {
        return Ok(PrefixAdmission::Nop);
    }
    // these compatibility encodings are nops in the fixed absent-feature profile.
    if matches!(instruction.code(), Code::Cldemote_m8 | Code::Rdsspd_r32)
        || ignored_instruction_prefetch(instruction)
    {
        if instruction.has_lock_prefix()
            || instruction.has_rep_prefix()
            || instruction.has_repne_prefix()
        {
            return Err(unsupported());
        }
        return Ok(PrefixAdmission::Nop);
    }
    Ok(PrefixAdmission::Instruction)
}

pub(super) fn check_scalar(instruction: &Instruction) -> Result<(), DecodeError> {
    let hle_hint = instruction.has_xacquire_prefix() || instruction.has_xrelease_prefix();
    let features = instruction.cpuid_features();
    let registers = (0..instruction.op_count())
        .filter(|&index| instruction.op_kind(index) == OpKind::Register)
        .map(|index| instruction.op_register(index));
    let x87_control = matches!(
        instruction.code(),
        Code::Fninit
            | Code::Fnclex
            | Code::Fnstsw_AX
            | Code::Fnstsw_m2byte
            | Code::Fnstcw_m2byte
            // obsolete compatibility opcodes are integer nops in modern x87 processors.
            | Code::Fneni
            | Code::Fndisi
            | Code::Fnsetpm
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
    // gcc emits rep ret as a predictor workaround; it keeps ordinary near-return semantics.
    let repeat_near_return = matches!(
        instruction.code(),
        Code::Retnw | Code::Retnd | Code::Retnw_imm16 | Code::Retnd_imm16
    );
    // hle is absent, so eligible hints retain the ordinary locked or store operation.
    if !hle_hint
        && !super::mpx::disabled_branch_prefix(instruction)
        && (instruction.has_repne_prefix()
            || instruction.has_rep_prefix() && !mandatory_repeat && !repeat_near_return)
    {
        return Err(unsupported());
    }
    // decoded operand width owns 66 semantics, including byte operations that ignore it.
    Ok(())
}

pub(super) fn has_repeat_prefix(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .take_while(|byte| is_prefix(**byte))
        .any(|byte| matches!(byte, 0xf2 | 0xf3))
}

// amd-compatible reserved memory types alias /0 while the wt1 facility is absent.
// this does not define the flags or cache behavior of an enabled wt1 instruction.
pub(super) fn ignored_prefetch_alias(instruction: &Instruction) -> bool {
    use crate::cpu::x86::capabilities::{CPU_ID_LEAVES, PREFETCHW_ENABLED};
    if !PREFETCHW_ENABLED {
        return false;
    }
    match instruction.code() {
        Code::Prefetchwt1_m8 => !CPU_ID_LEAVES.iter().any(|leaf| {
            leaf.function == 7 && leaf.subfunction == Some(0) && leaf.registers[2] & 1 != 0
        }),
        Code::Prefetchreserved3_m8
        | Code::Prefetchreserved4_m8
        | Code::Prefetchreserved5_m8
        | Code::Prefetchreserved6_m8
        | Code::Prefetchreserved7_m8 => true,
        _ => false,
    }
}

fn is_prefix(byte: u8) -> bool {
    matches!(
        byte,
        0xf0 | 0xf2 | 0xf3 | 0x66 | 0x67 | 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65
    )
}

#[cfg(test)]
#[path = "profile/tests/hle.rs"]
mod hle;

// only the two exact absent instruction-prefetch encodings are ignored.
// no operand address is needed and prefetchi remains unadvertised.
pub(super) fn ignored_instruction_prefetch(instruction: &Instruction) -> bool {
    use crate::cpu::x86::capabilities::CPU_ID_LEAVES;
    matches!(
        instruction.code(),
        Code::Prefetchit0_m8 | Code::Prefetchit1_m8
    ) && !CPU_ID_LEAVES.iter().any(|leaf| {
        leaf.function == 7 && leaf.subfunction == Some(1) && leaf.registers[3] & (1 << 14) != 0
    })
}

#[cfg(test)]
#[path = "profile/tests/prefetch/instruction.rs"]
mod instruction_prefetch;

// the fixed software profile selects these amd-documented modrm nops.
pub(super) fn legacy_nop_alias(instruction: &Instruction) -> bool {
    matches!(
        instruction.code(),
        Code::Reservednop_rm16_r16_0F19
            | Code::Reservednop_rm32_r32_0F19
            | Code::Reservednop_rm16_r16_0F1C
            | Code::Reservednop_rm32_r32_0F1C
            | Code::Reservednop_rm16_r16_0F1D
            | Code::Reservednop_rm32_r32_0F1D
            | Code::Reservednop_rm16_r16_0F1E
            | Code::Reservednop_rm32_r32_0F1E
            | Code::Reservednop_rm16_r16_0F1F
            | Code::Reservednop_rm32_r32_0F1F
    )
}

#[cfg(test)]
#[path = "profile/tests/nop.rs"]
mod legacy_nop;
