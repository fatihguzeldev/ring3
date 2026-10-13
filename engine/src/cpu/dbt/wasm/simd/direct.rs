mod multiply;
mod rearrange;
mod shift;

use super::super::{cpu_result, locals::*};
use super::{Imports, SimdKind, SimdOperand, SimdOperation, SimdRegister, mark_mmx, memory};
use crate::{
    abi::{
        arena::{SIMD_OFFSET, TRANSFER_OFFSET, X87_OFFSET},
        x86::{SIMD_XMM_OFFSET, X87_REGISTERS_OFFSET},
    },
    cpu::x86::simd::{CryptoKind, PackedKind as P, ShuffleKind as S},
};
use wasm_encoder::{BlockType, InstructionSink, MemArg, ValType};

pub(super) fn accepts(op: SimdOperation) -> bool {
    match op.kind {
        SimdKind::Move | SimdKind::MoveScalar => true,
        SimdKind::Crypto(CryptoKind::CarrylessMultiply) => true,
        SimdKind::Packed(kind) => matches!(
            kind,
            P::Add
                | P::Subtract
                | P::AddSignedSaturate
                | P::AddUnsignedSaturate
                | P::SubtractSignedSaturate
                | P::SubtractUnsignedSaturate
                | P::And
                | P::AndNot
                | P::Or
                | P::Xor
                | P::Equal
                | P::GreaterSigned
                | P::MultiplyLow
                | P::MultiplyHighSigned
                | P::MultiplyHighUnsigned
                | P::MultiplyUnsignedDword
                | P::MultiplyAddWords
                | P::AverageUnsigned
                | P::MinimumSigned
                | P::MaximumSigned
                | P::MinimumUnsigned
                | P::MaximumUnsigned
                | P::SumAbsoluteByteDifferences
                | P::ShiftLeft
                | P::ShiftRight
                | P::ShiftRightSigned
                | P::Absolute
                | P::Sign
                | P::HorizontalAdd
                | P::HorizontalSubtract
                | P::HorizontalAddSignedSaturate
                | P::HorizontalSubtractSignedSaturate
                | P::MultiplyAddUnsignedSignedBytes
                | P::MultiplyHighRoundSigned
                | P::MultiplySignedDword
                | P::MultipleSumAbsoluteByteDifferences
                | P::MinimumPositionUnsignedWord
                | P::Test
        ),
        SimdKind::Shuffle(S::PackSigned) => matches!(op.lane_bytes, 1 | 2),
        SimdKind::Shuffle(S::PackUnsigned) => matches!(op.lane_bytes, 1 | 2),
        SimdKind::Shuffle(kind) => matches!(
            kind,
            S::Dwords
                | S::Words
                | S::HighWords
                | S::LowWords
                | S::Singles
                | S::Doubles
                | S::UnpackLow
                | S::UnpackHigh
                | S::ShiftBytesLeft
                | S::ShiftBytesRight
                | S::InsertWord
                | S::MoveLowToHigh
                | S::MoveHighToLow
                | S::MaskBytes
                | S::MaskSingles
                | S::MaskDoubles
                | S::ExtractWord
                | S::Bytes
                | S::AlignRight
                | S::DuplicateEven
                | S::DuplicateOdd
                | S::SignExtendBytes
                | S::SignExtendWords
                | S::SignExtendDwords
                | S::ZeroExtendBytes
                | S::ZeroExtendWords
                | S::ZeroExtendDwords
                | S::BlendImmediate
                | S::BlendVariable
                | S::InsertByte
                | S::InsertDword
                | S::InsertSingle
                | S::ExtractByte
                | S::ExtractDword
        ),
        _ => false,
    }
}

fn arg(offset: usize) -> MemArg {
    MemArg {
        offset: offset as u64,
        align: 0,
        memory_index: 0,
    }
}

fn carryless_multiply(code: &mut InstructionSink<'_>, op: SimdOperation) {
    let immediate = op.immediate.unwrap();
    load(code, op.destination, 16);
    code.i64x2_extract_lane(immediate & 1)
        .local_set(SIMD_LHS_LOW);
    load(code, op.source, 16);
    code.i64x2_extract_lane(immediate >> 4 & 1)
        .local_set(SIMD_RHS)
        .i64_const(0)
        .local_set(SIMD_LHS_HIGH)
        .i64_const(0)
        .local_set(SIMD_RESULT_LOW)
        .i64_const(0)
        .local_set(SIMD_RESULT_HIGH)
        .i32_const(64)
        .local_set(ADDRESS)
        .loop_(BlockType::Empty);
    for (result, left) in [
        (SIMD_RESULT_LOW, SIMD_LHS_LOW),
        (SIMD_RESULT_HIGH, SIMD_LHS_HIGH),
    ] {
        code.local_get(result)
            .local_get(left)
            .i64_const(0)
            .local_get(SIMD_RHS)
            .i64_const(1)
            .i64_and()
            .i64_sub()
            .i64_and()
            .i64_xor()
            .local_set(result);
    }
    code.local_get(SIMD_LHS_HIGH)
        .i64_const(1)
        .i64_shl()
        .local_get(SIMD_LHS_LOW)
        .i64_const(63)
        .i64_shr_u()
        .i64_or()
        .local_set(SIMD_LHS_HIGH)
        .local_get(SIMD_LHS_LOW)
        .i64_const(1)
        .i64_shl()
        .local_set(SIMD_LHS_LOW)
        .local_get(SIMD_RHS)
        .i64_const(1)
        .i64_shr_u()
        .local_set(SIMD_RHS)
        .local_get(ADDRESS)
        .i32_const(1)
        .i32_sub()
        .local_tee(ADDRESS)
        .br_if(0)
        .end()
        .v128_const(0)
        .local_get(SIMD_RESULT_LOW)
        .i64x2_replace_lane(0)
        .local_get(SIMD_RESULT_HIGH)
        .i64x2_replace_lane(1);
}
fn load(code: &mut InstructionSink<'_>, operand: SimdOperand, width: u8) {
    if let SimdOperand::Register(SimdRegister::Gpr(gpr)) = operand {
        code.v128_const(0)
            .local_get(register(gpr))
            .i32x4_replace_lane(0);
        return;
    }
    let offset = match operand {
        SimdOperand::Register(SimdRegister::Xmm(index)) => {
            SIMD_OFFSET + SIMD_XMM_OFFSET + usize::from(index) * 16
        }
        SimdOperand::Register(SimdRegister::Mmx(index)) => {
            X87_OFFSET + X87_REGISTERS_OFFSET + usize::from(index) * 10
        }
        SimdOperand::Memory(_) => TRANSFER_OFFSET,
        _ => unreachable!("direct SIMD operands were decoded"),
    };
    if matches!(width, 1 | 2) {
        code.v128_const(0).local_get(STATE_PTR);
        if width == 1 {
            code.i32_load8_u(arg(offset));
        } else {
            code.i32_load16_u(arg(offset));
        }
        code.i32x4_replace_lane(0);
        return;
    }
    code.local_get(STATE_PTR);
    match width {
        4 => {
            code.v128_load32_zero(arg(offset));
        }
        8 => {
            code.v128_load64_zero(arg(offset));
        }
        16 => {
            code.v128_load(arg(offset));
        }
        _ => unreachable!(),
    }
}

fn arithmetic(code: &mut InstructionSink<'_>, kind: P, width: u8) {
    match (kind, width) {
        (P::Add, 1) => {
            code.i8x16_add();
        }
        (P::Add, 2) => {
            code.i16x8_add();
        }
        (P::Add, 4) => {
            code.i32x4_add();
        }
        (P::Add, 8) => {
            code.i64x2_add();
        }
        (P::Subtract, 1) => {
            code.i8x16_sub();
        }
        (P::Subtract, 2) => {
            code.i16x8_sub();
        }
        (P::Subtract, 4) => {
            code.i32x4_sub();
        }
        (P::Subtract, 8) => {
            code.i64x2_sub();
        }
        (P::AddSignedSaturate, 1) => {
            code.i8x16_add_sat_s();
        }
        (P::AddSignedSaturate, 2) => {
            code.i16x8_add_sat_s();
        }
        (P::AddUnsignedSaturate, 1) => {
            code.i8x16_add_sat_u();
        }
        (P::AddUnsignedSaturate, 2) => {
            code.i16x8_add_sat_u();
        }
        (P::SubtractSignedSaturate, 1) => {
            code.i8x16_sub_sat_s();
        }
        (P::SubtractSignedSaturate, 2) => {
            code.i16x8_sub_sat_s();
        }
        (P::SubtractUnsignedSaturate, 1) => {
            code.i8x16_sub_sat_u();
        }
        (P::SubtractUnsignedSaturate, 2) => {
            code.i16x8_sub_sat_u();
        }
        (P::And | P::AndNot, _) => {
            code.v128_and();
        }
        (P::Or, _) => {
            code.v128_or();
        }
        (P::Xor, _) => {
            code.v128_xor();
        }
        (P::Equal, 1) => {
            code.i8x16_eq();
        }
        (P::Equal, 2) => {
            code.i16x8_eq();
        }
        (P::Equal, 4) => {
            code.i32x4_eq();
        }
        (P::Equal, 8) => {
            code.i64x2_eq();
        }
        (P::GreaterSigned, 1) => {
            code.i8x16_gt_s();
        }
        (P::GreaterSigned, 2) => {
            code.i16x8_gt_s();
        }
        (P::GreaterSigned, 4) => {
            code.i32x4_gt_s();
        }
        (P::GreaterSigned, 8) => {
            code.i64x2_gt_s();
        }
        (P::MultiplyLow, 2) => {
            code.i16x8_mul();
        }
        (P::MultiplyLow, 4) => {
            code.i32x4_mul();
        }
        (P::MultiplyAddWords, 2) => {
            code.i32x4_dot_i16x8_s();
        }
        (P::AverageUnsigned, 1) => {
            code.i8x16_avgr_u();
        }
        (P::AverageUnsigned, 2) => {
            code.i16x8_avgr_u();
        }
        (P::MinimumSigned, 1) => {
            code.i8x16_min_s();
        }
        (P::MinimumSigned, 4) => {
            code.i32x4_min_s();
        }
        (P::MaximumSigned, 1) => {
            code.i8x16_max_s();
        }
        (P::MaximumSigned, 4) => {
            code.i32x4_max_s();
        }
        (P::MinimumUnsigned, 2) => {
            code.i16x8_min_u();
        }
        (P::MinimumUnsigned, 4) => {
            code.i32x4_min_u();
        }
        (P::MaximumUnsigned, 2) => {
            code.i16x8_max_u();
        }
        (P::MaximumUnsigned, 4) => {
            code.i32x4_max_u();
        }
        (P::MinimumSigned, 2) => {
            code.i16x8_min_s();
        }
        (P::MaximumSigned, 2) => {
            code.i16x8_max_s();
        }
        (P::MinimumUnsigned, 1) => {
            code.i8x16_min_u();
        }
        (P::MaximumUnsigned, 1) => {
            code.i8x16_max_u();
        }
        (P::ShiftLeft, 2) => {
            code.i16x8_shl();
        }
        (P::ShiftLeft, 4) => {
            code.i32x4_shl();
        }
        (P::ShiftLeft, 8) => {
            code.i64x2_shl();
        }
        (P::ShiftRight, 2) => {
            code.i16x8_shr_u();
        }
        (P::ShiftRight, 4) => {
            code.i32x4_shr_u();
        }
        (P::ShiftRight, 8) => {
            code.i64x2_shr_u();
        }
        (P::ShiftRightSigned, 2) => {
            code.i16x8_shr_s();
        }
        (P::ShiftRightSigned, 4) => {
            code.i32x4_shr_s();
        }
        _ => unreachable!("direct packed opcode and width were decoded"),
    }
}

fn multiply_dword(code: &mut InstructionSink<'_>, op: SimdOperation) {
    multiply::multiply_dword(
        code,
        op.kind == SimdKind::Packed(P::MultiplySignedDword),
        |code, second| {
            load(
                code,
                if second { op.source } else { op.destination },
                op.vector_bytes,
            )
        },
    );
}

fn sum_absolute_byte_differences(code: &mut InstructionSink<'_>, op: SimdOperation) {
    multiply::sum_absolute_byte_differences(code, |code, second| {
        load(
            code,
            if second { op.source } else { op.destination },
            op.vector_bytes,
        )
    });
}

fn multiply_high(code: &mut InstructionSink<'_>, op: SimdOperation, kind: P) {
    multiply::multiply_high(code, kind, op.vector_bytes, |code, second| {
        load(
            code,
            if second { op.source } else { op.destination },
            op.vector_bytes,
        )
    });
}

fn absolute(code: &mut InstructionSink<'_>, width: u8) {
    match width {
        1 => {
            code.i8x16_abs();
        }
        2 => {
            code.i16x8_abs();
        }
        4 => {
            code.i32x4_abs();
        }
        _ => unreachable!(),
    }
}

#[cfg(test)]
pub(super) fn integer_lanes(
    code: &mut InstructionSink<'_>,
    operation: crate::cpu::x86::simd::IntegerLaneOperation,
    scratch: [u32; 2],
) {
    let width = operation.lane_bytes();
    match operation.kind() {
        P::Absolute => absolute(code, width),
        P::Sign => {
            let [first, second] = scratch;
            code.local_set(second).local_set(first).local_get(first);
            match width {
                1 => {
                    code.i8x16_neg();
                }
                2 => {
                    code.i16x8_neg();
                }
                4 => {
                    code.i32x4_neg();
                }
                _ => unreachable!(),
            }
            code.local_get(first).local_get(second).v128_const(0);
            match width {
                1 => {
                    code.i8x16_lt_s();
                }
                2 => {
                    code.i16x8_lt_s();
                }
                4 => {
                    code.i32x4_lt_s();
                }
                _ => unreachable!(),
            }
            code.v128_bitselect().local_get(second).v128_const(0);
            arithmetic(code, P::Equal, width);
            code.v128_not().v128_and();
        }
        kind => arithmetic(code, kind, width),
    }
}

fn sign(code: &mut InstructionSink<'_>, op: SimdOperation) {
    load(code, op.destination, op.vector_bytes);
    match op.lane_bytes {
        1 => {
            code.i8x16_neg();
        }
        2 => {
            code.i16x8_neg();
        }
        4 => {
            code.i32x4_neg();
        }
        _ => unreachable!(),
    }
    load(code, op.destination, op.vector_bytes);
    load(code, op.source, op.vector_bytes);
    code.v128_const(0);
    match op.lane_bytes {
        1 => {
            code.i8x16_lt_s();
        }
        2 => {
            code.i16x8_lt_s();
        }
        4 => {
            code.i32x4_lt_s();
        }
        _ => unreachable!(),
    }
    code.v128_bitselect();
    load(code, op.source, op.vector_bytes);
    code.v128_const(0);
    arithmetic(code, P::Equal, op.lane_bytes);
    code.v128_not().v128_and();
}

fn horizontal(code: &mut InstructionSink<'_>, op: SimdOperation, kind: P) {
    multiply::horizontal(
        code,
        kind,
        op.vector_bytes,
        op.lane_bytes,
        |code, second| {
            load(
                code,
                if second { op.source } else { op.destination },
                op.vector_bytes,
            )
        },
    );
}

fn multiply_add_unsigned_signed_bytes(code: &mut InstructionSink<'_>, op: SimdOperation) {
    multiply::multiply_add_unsigned_signed_bytes(code, |code, second| {
        load(
            code,
            if second { op.source } else { op.destination },
            op.vector_bytes,
        )
    });
}

fn multiply_high_round_signed(code: &mut InstructionSink<'_>, op: SimdOperation) {
    multiply::multiply_high_round_signed(code, |code, second| {
        load(
            code,
            if second { op.source } else { op.destination },
            op.vector_bytes,
        )
    });
}

fn shuffle_bytes(code: &mut InstructionSink<'_>, op: SimdOperation) {
    load(code, op.destination, op.vector_bytes);
    load(code, op.source, op.vector_bytes);
    rearrange::bytes(code, op.vector_bytes, |code| {
        load(code, op.source, op.vector_bytes)
    });
}

fn align_right(code: &mut InstructionSink<'_>, op: SimdOperation) {
    let size = usize::from(op.vector_bytes);
    let count = usize::from(op.immediate.unwrap());
    if count >= size * 2 {
        code.v128_const(0);
        return;
    }
    load(code, op.source, op.vector_bytes);
    load(code, op.destination, op.vector_bytes);
    rearrange::align_right(code, size, count);
}

fn packed_shift(code: &mut InstructionSink<'_>, op: SimdOperation, kind: P) {
    let count = |code: &mut InstructionSink<'_>| {
        if let Some(immediate) = op.immediate {
            code.i64_const(i64::from(immediate));
        } else {
            load(code, op.source, 8);
            code.i64x2_extract_lane(0);
        }
    };
    shift::lanes(
        code,
        kind,
        op.lane_bytes,
        |code| load(code, op.destination, op.vector_bytes),
        count,
    );
}

fn saturating_pack(code: &mut InstructionSink<'_>, op: SimdOperation, kind: S) {
    load(code, op.destination, op.vector_bytes);
    load(code, op.source, op.vector_bytes);
    if op.vector_bytes == 8 {
        // combine both mmx halves before narrowing so padding follows the source lanes.
        code.i8x16_shuffle([0, 1, 2, 3, 4, 5, 6, 7, 16, 17, 18, 19, 20, 21, 22, 23])
            .v128_const(0);
    }
    rearrange::pack(code, kind, op.lane_bytes);
}

fn byte_shift(code: &mut InstructionSink<'_>, op: SimdOperation, kind: S) {
    shift::bytes(code, kind, op.immediate.unwrap(), |code| {
        load(code, op.destination, 16)
    });
}

fn multiple_sum_absolute_byte_differences(code: &mut InstructionSink<'_>, op: SimdOperation) {
    multiply::multiple_sum_absolute_byte_differences(
        code,
        op.immediate.unwrap(),
        |code, second| load(code, if second { op.source } else { op.destination }, 16),
    );
}

fn minimum_position_unsigned_word(code: &mut InstructionSink<'_>, op: SimdOperation) {
    multiply::minimum_position_unsigned_word(code, [RESULT, RHS, LHS], |code| {
        load(code, op.source, 16)
    });
}

fn packed_test(code: &mut InstructionSink<'_>, op: SimdOperation) {
    load(code, op.destination, 16);
    load(code, op.source, 16);
    code.v128_and()
        .v128_any_true()
        .i32_eqz()
        .i32_const(6)
        .i32_shl()
        .local_set(RESULT);
    load(code, op.destination, 16);
    code.v128_not();
    load(code, op.source, 16);
    code.v128_and()
        .v128_any_true()
        .i32_eqz()
        .local_get(RESULT)
        .i32_or()
        .local_get(FLAGS)
        .i32_const(!0x8d5)
        .i32_and()
        .i32_or()
        .local_set(FLAGS);
}

fn insert_element(code: &mut InstructionSink<'_>, op: SimdOperation) {
    load(code, op.destination, op.vector_bytes);
    match op.source {
        SimdOperand::Register(SimdRegister::Gpr(gpr)) => {
            code.local_get(register(gpr));
        }
        SimdOperand::Memory(_) => {
            code.local_get(STATE_PTR);
            match op.lane_bytes {
                1 => {
                    code.i32_load8_u(arg(TRANSFER_OFFSET));
                }
                2 => {
                    code.i32_load16_u(arg(TRANSFER_OFFSET));
                }
                4 => {
                    code.i32_load(arg(TRANSFER_OFFSET));
                }
                _ => unreachable!("direct insertion width was decoded"),
            }
        }
        _ => unreachable!("direct insertion operands were decoded"),
    }
    let lane = op.immediate.unwrap() % (op.vector_bytes / op.lane_bytes);
    match op.lane_bytes {
        1 => {
            code.i8x16_replace_lane(lane);
        }
        2 => {
            code.i16x8_replace_lane(lane);
        }
        4 => {
            code.i32x4_replace_lane(lane);
        }
        _ => unreachable!("direct insertion width was decoded"),
    }
}

fn insert_single(code: &mut InstructionSink<'_>, op: SimdOperation) {
    let immediate = op.immediate.unwrap();
    load(code, op.destination, 16);
    load(code, op.source, if op.reads_memory() { 4 } else { 16 });
    code.i32x4_extract_lane(if op.reads_memory() { 0 } else { immediate >> 6 })
        .i32x4_replace_lane(immediate >> 4 & 3);
    let mask = std::array::from_fn(|byte| {
        if immediate & (1 << (byte / 4)) != 0 {
            0
        } else {
            0xff
        }
    });
    code.v128_const(i128::from_le_bytes(mask)).v128_and();
}

fn widen(code: &mut InstructionSink<'_>, op: SimdOperation, kind: S) {
    let (from, _) = kind.widening().unwrap();
    load(code, op.source, 16 / op.lane_bytes * from);
    rearrange::widen(code, kind, op.lane_bytes);
}

fn blend_variable(code: &mut InstructionSink<'_>, op: SimdOperation) {
    load(code, op.source, 16);
    load(code, op.destination, 16);
    load(code, SimdOperand::Register(SimdRegister::Xmm(0)), 16);
    rearrange::blend_variable(code, op.lane_bytes);
}

fn shuffle(code: &mut InstructionSink<'_>, op: SimdOperation, kind: S) {
    let source_only = matches!(
        kind,
        S::Dwords | S::Words | S::HighWords | S::LowWords | S::DuplicateEven | S::DuplicateOdd
    );
    load(
        code,
        if source_only {
            op.source
        } else {
            op.destination
        },
        op.vector_bytes,
    );
    load(code, op.source, op.vector_bytes);
    rearrange::shuffle(
        code,
        kind,
        op.vector_bytes,
        op.lane_bytes,
        op.immediate.unwrap_or(0),
    );
}

pub(super) fn instruction(
    code: &mut InstructionSink<'_>,
    op: SimdOperation,
    imports: Imports,
    exit_depth: u32,
) -> bool {
    let alignment = if op.aligned {
        if op.vector_bytes == 8 { 8 } else { 16 }
    } else {
        1
    };
    if let SimdOperand::Memory(address) = if op.reads_memory() {
        op.source
    } else {
        op.destination
    } {
        memory::checked_address(
            code,
            address,
            u32::from(op.memory_bytes),
            if op.writes_memory() {
                crate::cpu::x86::SegmentAccess::Write
            } else {
                crate::cpu::x86::SegmentAccess::Read
            },
            imports,
            exit_depth,
        );
    }
    if op.reads_memory() {
        code.local_get(ADDRESS)
            .i32_const(i32::from(op.memory_bytes))
            .i32_const(alignment)
            .call(imports.cpu_read.expect("direct SIMD read"))
            .local_set(HELPER_STATUS);
        cpu_result::validate(code, u32::from(op.memory_bytes), 1, false, exit_depth);
    }
    if op.kind == SimdKind::Packed(P::Test) {
        packed_test(code, op);
        return false;
    }
    let scalar_gpr = matches!(op.destination, SimdOperand::Register(SimdRegister::Gpr(_)));
    if !scalar_gpr {
        code.local_get(STATE_PTR);
    }
    match op.kind {
        SimdKind::Crypto(CryptoKind::CarrylessMultiply) => carryless_multiply(code, op),
        SimdKind::Move => load(code, op.source, op.lane_bytes),
        SimdKind::MoveScalar => {
            if op.reads_memory() || op.writes_memory() {
                load(code, op.source, op.lane_bytes);
            } else {
                load(code, op.destination, 16);
                load(code, op.source, op.lane_bytes);
                match op.lane_bytes {
                    4 => {
                        code.i32x4_extract_lane(0).i32x4_replace_lane(0);
                    }
                    8 => {
                        code.i64x2_extract_lane(0).i64x2_replace_lane(0);
                    }
                    _ => unreachable!("scalar SIMD move width was decoded"),
                }
            }
        }
        SimdKind::Packed(P::MultipleSumAbsoluteByteDifferences) => {
            multiple_sum_absolute_byte_differences(code, op)
        }
        SimdKind::Packed(P::MinimumPositionUnsignedWord) => {
            minimum_position_unsigned_word(code, op)
        }
        SimdKind::Packed(P::Absolute) => {
            load(code, op.source, op.vector_bytes);
            absolute(code, op.lane_bytes);
        }
        SimdKind::Packed(P::Sign) => sign(code, op),
        SimdKind::Packed(kind)
            if matches!(
                kind,
                P::HorizontalAdd
                    | P::HorizontalSubtract
                    | P::HorizontalAddSignedSaturate
                    | P::HorizontalSubtractSignedSaturate
            ) =>
        {
            horizontal(code, op, kind)
        }
        SimdKind::Packed(P::MultiplyAddUnsignedSignedBytes) => {
            multiply_add_unsigned_signed_bytes(code, op)
        }
        SimdKind::Packed(P::MultiplyHighRoundSigned) => multiply_high_round_signed(code, op),
        SimdKind::Shuffle(S::Bytes) => shuffle_bytes(code, op),
        SimdKind::Shuffle(S::AlignRight) => align_right(code, op),
        SimdKind::Packed(P::MultiplyUnsignedDword | P::MultiplySignedDword) => {
            multiply_dword(code, op)
        }
        SimdKind::Packed(P::SumAbsoluteByteDifferences) => sum_absolute_byte_differences(code, op),
        SimdKind::Packed(kind)
            if matches!(kind, P::MultiplyHighSigned | P::MultiplyHighUnsigned) =>
        {
            multiply_high(code, op, kind);
        }
        SimdKind::Packed(kind)
            if matches!(kind, P::ShiftLeft | P::ShiftRight | P::ShiftRightSigned) =>
        {
            packed_shift(code, op, kind);
        }
        SimdKind::Packed(kind) => {
            load(code, op.destination, op.vector_bytes);
            if kind == P::AndNot {
                code.v128_not();
            }
            load(code, op.source, op.vector_bytes);
            arithmetic(code, kind, op.lane_bytes);
        }
        SimdKind::Shuffle(kind) if kind.widening().is_some() => widen(code, op, kind),
        SimdKind::Shuffle(S::BlendVariable) => blend_variable(code, op),
        SimdKind::Shuffle(S::InsertSingle) => insert_single(code, op),
        SimdKind::Shuffle(kind) if matches!(kind, S::PackSigned | S::PackUnsigned) => {
            saturating_pack(code, op, kind);
        }
        SimdKind::Shuffle(kind)
            if matches!(
                kind,
                S::MaskBytes
                    | S::MaskSingles
                    | S::MaskDoubles
                    | S::ExtractByte
                    | S::ExtractWord
                    | S::ExtractDword
            ) =>
        {
            load(code, op.source, op.vector_bytes);
            match kind {
                S::MaskBytes => {
                    code.i8x16_bitmask();
                }
                S::MaskSingles => {
                    code.i32x4_bitmask();
                }
                S::MaskDoubles => {
                    code.i64x2_bitmask();
                }
                S::ExtractByte => {
                    code.i8x16_extract_lane_u(op.immediate.unwrap() % 16);
                }
                S::ExtractWord => {
                    code.i16x8_extract_lane_u(op.immediate.unwrap() % (op.vector_bytes / 2));
                }
                S::ExtractDword => {
                    code.i32x4_extract_lane(op.immediate.unwrap() % 4);
                }
                _ => unreachable!(),
            }
        }
        SimdKind::Shuffle(kind) if matches!(kind, S::ShiftBytesLeft | S::ShiftBytesRight) => {
            byte_shift(code, op, kind);
        }
        SimdKind::Shuffle(S::InsertByte | S::InsertWord | S::InsertDword) => {
            insert_element(code, op)
        }
        SimdKind::Shuffle(kind) => shuffle(code, op, kind),
        _ => unreachable!(),
    }
    match op.destination {
        SimdOperand::Register(SimdRegister::Xmm(index)) => {
            code.v128_store(arg(SIMD_OFFSET + SIMD_XMM_OFFSET + usize::from(index) * 16));
        }
        SimdOperand::Register(SimdRegister::Mmx(index)) => {
            let offset = X87_OFFSET + X87_REGISTERS_OFFSET + usize::from(index) * 10;
            code.v128_store64_lane(arg(offset), 0);
            code.local_get(STATE_PTR)
                .i32_const(0xffff)
                .i32_store16(arg(offset + 8));
        }
        SimdOperand::Register(SimdRegister::Gpr(gpr)) => {
            if op.kind == SimdKind::Move {
                code.i32x4_extract_lane(0);
            }
            code.local_set(register(gpr));
        }
        SimdOperand::Memory(_) => {
            match (op.kind, op.lane_bytes) {
                (SimdKind::Shuffle(S::ExtractByte), 1) => {
                    code.i32_store8(arg(TRANSFER_OFFSET));
                }
                (SimdKind::Shuffle(S::ExtractWord), 2) => {
                    code.i32_store16(arg(TRANSFER_OFFSET));
                }
                (SimdKind::Shuffle(S::ExtractDword), 4) => {
                    code.i32_store(arg(TRANSFER_OFFSET));
                }
                (SimdKind::MoveScalar, 4) => {
                    code.v128_store32_lane(arg(TRANSFER_OFFSET), 0);
                }
                (SimdKind::MoveScalar, 8) => {
                    code.v128_store64_lane(arg(TRANSFER_OFFSET), 0);
                }
                _ => {
                    code.v128_store(arg(TRANSFER_OFFSET));
                }
            }
            let import = imports.cpu_write.expect("direct SIMD write");
            let index = import.emit_binding(code);
            code.local_get(ADDRESS)
                .i32_const(i32::from(op.memory_bytes))
                .i32_const(alignment)
                .call(index)
                .local_set(HELPER_STATUS);
            cpu_result::validate(code, u32::from(op.memory_bytes), 2, true, exit_depth);
        }
        _ => unreachable!(),
    }
    if op.uses_mmx() {
        mark_mmx(code);
    }
    op.writes_memory()
}

#[cfg(test)]
pub(super) fn rearrange_value(
    code: &mut InstructionSink<'_>,
    operation: crate::cpu::x86::simd::RearrangeOperation,
    sources: [u32; 3],
) {
    let [first, second, mask] = sources;
    let kind = operation.kind();
    match kind {
        S::PackSigned | S::PackUnsigned => {
            code.local_get(first).local_get(second);
            rearrange::pack(code, kind, operation.lane_bytes());
        }
        _ if kind.widening().is_some() => {
            code.local_get(second);
            rearrange::widen(code, kind, operation.lane_bytes());
        }
        S::Bytes => {
            code.local_get(first).local_get(second);
            rearrange::bytes(code, 16, |code| {
                code.local_get(second);
            });
        }
        S::AlignRight => {
            let count = usize::from(operation.immediate());
            if count >= 32 {
                code.v128_const(0);
            } else {
                code.local_get(second).local_get(first);
                rearrange::align_right(code, 16, count);
            }
        }
        S::BlendVariable => {
            code.local_get(second).local_get(first).local_get(mask);
            rearrange::blend_variable(code, 1);
        }
        _ => {
            code.local_get(if operation.unary() { second } else { first })
                .local_get(second);
            rearrange::shuffle(
                code,
                kind,
                16,
                operation.lane_bytes(),
                operation.immediate(),
            );
        }
    }
}

#[cfg(test)]
pub(super) fn shift_value(
    code: &mut InstructionSink<'_>,
    operation: crate::cpu::x86::simd::ShiftOperation,
    immediate: Option<u8>,
    sources: [u32; 2],
) {
    let [source, count] = sources;
    if let Some(kind) = operation.byte_kind() {
        shift::bytes(code, kind, immediate.unwrap(), |code| {
            code.local_get(source);
        });
    } else {
        shift::lanes(
            code,
            operation.lane_kind().unwrap(),
            operation.lane_bytes(),
            |code| {
                code.local_get(source);
            },
            |code| {
                if let Some(n) = immediate {
                    code.i64_const(i64::from(n));
                } else {
                    code.local_get(count).i64x2_extract_lane(0);
                }
            },
        );
    }
}

#[cfg(test)]
pub(super) fn multiply_value(
    code: &mut InstructionSink<'_>,
    operation: crate::cpu::x86::simd::MultiplyOperation,
    sources: [u32; 2],
    scratch: [u32; 3],
) {
    let [first, second] = sources;
    let load_source = |code: &mut InstructionSink<'_>, b: bool| {
        code.local_get(if b { second } else { first });
    };
    let kind = operation.kind();
    match kind {
        P::MultiplyLow | P::MultiplyAddWords => {
            load_source(code, false);
            load_source(code, true);
            arithmetic(code, kind, operation.lane_bytes());
        }
        P::MultiplyUnsignedDword | P::MultiplySignedDword => {
            multiply::multiply_dword(code, kind == P::MultiplySignedDword, load_source)
        }
        P::MultiplyHighSigned | P::MultiplyHighUnsigned => {
            multiply::multiply_high(code, kind, 16, load_source)
        }
        P::SumAbsoluteByteDifferences => multiply::sum_absolute_byte_differences(code, load_source),
        P::HorizontalAdd
        | P::HorizontalSubtract
        | P::HorizontalAddSignedSaturate
        | P::HorizontalSubtractSignedSaturate => {
            multiply::horizontal(code, kind, 16, operation.lane_bytes(), load_source)
        }
        P::MultiplyAddUnsignedSignedBytes => {
            multiply::multiply_add_unsigned_signed_bytes(code, load_source)
        }
        P::MultiplyHighRoundSigned => multiply::multiply_high_round_signed(code, load_source),
        P::MultipleSumAbsoluteByteDifferences => multiply::multiple_sum_absolute_byte_differences(
            code,
            operation.immediate(),
            load_source,
        ),
        P::MinimumPositionUnsignedWord => {
            multiply::minimum_position_unsigned_word(code, scratch, |code| {
                code.local_get(second);
            })
        }
        _ => unreachable!("validated multiply operation"),
    }
}
