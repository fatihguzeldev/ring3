use super::super::{cpu_result, locals::*};
use super::{Imports, SimdKind, SimdOperand, SimdOperation, SimdRegister, mark_mmx, memory};
use crate::{
    abi::{
        arena::{SIMD_OFFSET, TRANSFER_OFFSET, X87_OFFSET},
        x86::{SIMD_XMM_OFFSET, X87_REGISTERS_OFFSET},
    },
    cpu::x86::simd::{PackedKind as P, ShuffleKind as S},
};
use wasm_encoder::{BlockType, InstructionSink, MemArg, ValType};

pub(super) fn accepts(op: SimdOperation) -> bool {
    match op.kind {
        SimdKind::Move | SimdKind::MoveScalar => true,
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
    for operand in [op.destination, op.source] {
        load(code, operand, op.vector_bytes);
        code.v128_const(0)
            .i8x16_shuffle([0, 1, 2, 3, 8, 9, 10, 11, 16, 17, 18, 19, 20, 21, 22, 23]);
    }
    if op.kind == SimdKind::Packed(P::MultiplySignedDword) {
        code.i64x2_extmul_low_i32x4_s();
    } else {
        code.i64x2_extmul_low_i32x4_u();
    }
}

fn sum_absolute_byte_differences(code: &mut InstructionSink<'_>, op: SimdOperation) {
    load(code, op.destination, op.vector_bytes);
    load(code, op.source, op.vector_bytes);
    code.i8x16_max_u();
    load(code, op.destination, op.vector_bytes);
    load(code, op.source, op.vector_bytes);
    code.i8x16_min_u()
        .i8x16_sub()
        .i16x8_extadd_pairwise_i8x16_u()
        .i32x4_extadd_pairwise_i16x8_u();
    // four-byte sums are at most 1020, so narrowing preserves them.
    code.v128_const(0)
        .i16x8_narrow_i32x4_u()
        .i32x4_extadd_pairwise_i16x8_u()
        .i64x2_extend_low_i32x4_u();
}

fn multiply_high(code: &mut InstructionSink<'_>, op: SimdOperation, kind: P) {
    let signed = kind == P::MultiplyHighSigned;
    for high in [false, true] {
        if high && op.vector_bytes == 8 {
            code.v128_const(0);
            continue;
        }
        load(code, op.destination, op.vector_bytes);
        load(code, op.source, op.vector_bytes);
        match (signed, high) {
            (true, false) => {
                code.i32x4_extmul_low_i16x8_s();
            }
            (true, true) => {
                code.i32x4_extmul_high_i16x8_s();
            }
            (false, false) => {
                code.i32x4_extmul_low_i16x8_u();
            }
            (false, true) => {
                code.i32x4_extmul_high_i16x8_u();
            }
        }
        code.i32_const(16);
        if signed {
            code.i32x4_shr_s();
        } else {
            code.i32x4_shr_u();
        }
    }
    if signed {
        code.i16x8_narrow_i32x4_s();
    } else {
        code.i16x8_narrow_i32x4_u();
    }
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
    let size = usize::from(op.vector_bytes);
    let width = usize::from(op.lane_bytes);
    for odd in [false, true] {
        load(code, op.destination, op.vector_bytes);
        load(code, op.source, op.vector_bytes);
        let lanes = std::array::from_fn(|byte| {
            if byte >= size {
                return 15;
            }
            let lane = byte / width;
            let source = if lane < size / width / 2 { 0 } else { 16 };
            (source + (lane % (size / width / 2) * 2 + usize::from(odd)) * width + byte % width)
                as u8
        });
        code.i8x16_shuffle(lanes);
    }
    arithmetic(
        code,
        match kind {
            P::HorizontalAdd => P::Add,
            P::HorizontalSubtract => P::Subtract,
            P::HorizontalAddSignedSaturate => P::AddSignedSaturate,
            P::HorizontalSubtractSignedSaturate => P::SubtractSignedSaturate,
            _ => unreachable!(),
        },
        op.lane_bytes,
    );
}

fn multiply_add_unsigned_signed_bytes(code: &mut InstructionSink<'_>, op: SimdOperation) {
    for high in [false, true] {
        load(code, op.destination, op.vector_bytes);
        if high {
            code.i16x8_extend_high_i8x16_u();
        } else {
            code.i16x8_extend_low_i8x16_u();
        }
        load(code, op.source, op.vector_bytes);
        if high {
            code.i16x8_extend_high_i8x16_s();
        } else {
            code.i16x8_extend_low_i8x16_s();
        }
        code.i16x8_mul().i32x4_extadd_pairwise_i16x8_s();
    }
    code.i16x8_narrow_i32x4_s();
}

fn multiply_high_round_signed(code: &mut InstructionSink<'_>, op: SimdOperation) {
    for high in [false, true] {
        load(code, op.destination, op.vector_bytes);
        load(code, op.source, op.vector_bytes);
        if high {
            code.i32x4_extmul_high_i16x8_s();
        } else {
            code.i32x4_extmul_low_i16x8_s();
        }
        code.i32_const(0x4000)
            .i32x4_splat()
            .i32x4_add()
            .i32_const(15)
            .i32x4_shr_s();
    }
    // x86 wraps +32768; wasm's saturating q15 multiply would return +32767.
    code.i8x16_shuffle([0, 1, 4, 5, 8, 9, 12, 13, 16, 17, 20, 21, 24, 25, 28, 29]);
}

fn shuffle_bytes(code: &mut InstructionSink<'_>, op: SimdOperation) {
    load(code, op.destination, op.vector_bytes);
    load(code, op.source, op.vector_bytes);
    code.i32_const(i32::from(op.vector_bytes) - 1)
        .i8x16_splat()
        .v128_and()
        .i8x16_swizzle();
    load(code, op.source, op.vector_bytes);
    code.i32_const(0).i8x16_splat().i8x16_ge_s().v128_and();
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
    let lanes = std::array::from_fn(|byte| {
        let index = count + byte;
        if index < size {
            index as u8
        } else if index < size * 2 {
            (16 + index - size) as u8
        } else {
            0
        }
    });
    code.i8x16_shuffle(lanes);
    let mask = std::array::from_fn::<_, 16, _>(|byte| {
        if byte < size && count + byte < size * 2 {
            255u8
        } else {
            0
        }
    });
    code.v128_const(i128::from_le_bytes(mask)).v128_and();
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
    let bits = i32::from(op.lane_bytes) * 8;
    count(code);
    code.i64_const(i64::from(bits))
        .i64_ge_u()
        .if_(BlockType::Result(ValType::V128));
    if kind == P::ShiftRightSigned {
        load(code, op.destination, op.vector_bytes);
        code.i32_const(bits - 1);
        arithmetic(code, kind, op.lane_bytes);
    } else {
        code.v128_const(0);
    }
    code.else_();
    load(code, op.destination, op.vector_bytes);
    count(code);
    code.i32_wrap_i64();
    arithmetic(code, kind, op.lane_bytes);
    code.end();
}

fn saturating_pack(code: &mut InstructionSink<'_>, op: SimdOperation, kind: S) {
    load(code, op.destination, op.vector_bytes);
    load(code, op.source, op.vector_bytes);
    if op.vector_bytes == 8 {
        // combine both mmx halves before narrowing so padding follows the source lanes.
        code.i8x16_shuffle([0, 1, 2, 3, 4, 5, 6, 7, 16, 17, 18, 19, 20, 21, 22, 23])
            .v128_const(0);
    }
    match (kind, op.lane_bytes) {
        (S::PackSigned, 1) => {
            code.i8x16_narrow_i16x8_s();
        }
        (S::PackSigned, 2) => {
            code.i16x8_narrow_i32x4_s();
        }
        (S::PackUnsigned, 1) => {
            code.i8x16_narrow_i16x8_u();
        }
        (S::PackUnsigned, 2) => {
            code.i16x8_narrow_i32x4_u();
        }
        _ => unreachable!("direct saturating pack kind and width were decoded"),
    }
}

fn byte_shift(code: &mut InstructionSink<'_>, op: SimdOperation, kind: S) {
    let count = usize::from(op.immediate.unwrap()).min(16);
    let lanes = std::array::from_fn(|i| {
        if kind == S::ShiftBytesLeft {
            if i >= count { (i - count) as u8 } else { 16 }
        } else if i + count < 16 {
            (i + count) as u8
        } else {
            16
        }
    });
    load(code, op.destination, 16);
    code.v128_const(0).i8x16_shuffle(lanes);
}

fn insert_word(code: &mut InstructionSink<'_>, op: SimdOperation) {
    load(code, op.destination, op.vector_bytes);
    match op.source {
        SimdOperand::Register(SimdRegister::Gpr(gpr)) => {
            code.local_get(register(gpr));
        }
        SimdOperand::Memory(_) => {
            code.local_get(STATE_PTR).i32_load16_u(arg(TRANSFER_OFFSET));
        }
        _ => unreachable!("direct word insertion operands were decoded"),
    }
    code.i16x8_replace_lane(op.immediate.unwrap() % (op.vector_bytes / 2));
}

fn shuffle(code: &mut InstructionSink<'_>, op: SimdOperation, kind: S) {
    let width = usize::from(op.lane_bytes);
    let size = usize::from(op.vector_bytes);
    let immediate = op.immediate.unwrap_or(0);
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
    let mut lanes = std::array::from_fn(|i| i as u8);
    match kind {
        S::DuplicateEven | S::DuplicateOdd => {
            for lane in 0..size / width {
                let source = (lane / 2 * 2 + usize::from(kind == S::DuplicateOdd)) * width;
                for byte in 0..width {
                    lanes[lane * width + byte] = (source + byte) as u8;
                }
            }
        }
        S::Dwords | S::Words => {
            for i in 0..4 {
                for j in 0..width {
                    lanes[i * width + j] =
                        (((immediate >> (i * 2)) & 3) as usize * width + j) as u8;
                }
            }
        }
        S::HighWords | S::LowWords => {
            let start = if kind == S::HighWords { 8 } else { 0 };
            for i in 0..4 {
                for j in 0..2 {
                    lanes[start + i * 2 + j] =
                        (start + usize::from((immediate >> (i * 2)) & 3) * 2 + j) as u8;
                }
            }
        }
        S::Singles => {
            for i in 0..4 {
                for j in 0..4 {
                    lanes[i * 4 + j] = (usize::from((immediate >> (i * 2)) & 3) * 4
                        + j
                        + if i < 2 { 0 } else { 16 }) as u8;
                }
            }
        }
        S::Doubles => {
            for j in 0..8 {
                lanes[j] = (usize::from(immediate & 1) * 8 + j) as u8;
                lanes[8 + j] = (16 + usize::from((immediate >> 1) & 1) * 8 + j) as u8;
            }
        }
        S::UnpackLow | S::UnpackHigh => {
            let start = if kind == S::UnpackHigh { size / 2 } else { 0 };
            for i in 0..size / width / 2 {
                for j in 0..width {
                    lanes[i * width * 2 + j] = (start + i * width + j) as u8;
                    lanes[i * width * 2 + width + j] = (16 + start + i * width + j) as u8;
                }
            }
        }
        S::MoveLowToHigh => {
            for j in 0..8 {
                lanes[8 + j] = (16 + j) as u8;
            }
        }
        S::MoveHighToLow => {
            for (j, lane) in lanes[..8].iter_mut().enumerate() {
                *lane = (24 + j) as u8;
            }
        }
        _ => unreachable!(),
    }
    code.i8x16_shuffle(lanes);
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
        memory::address_value(code, address);
        code.local_set(ADDRESS);
    }
    if op.reads_memory() {
        code.local_get(ADDRESS)
            .i32_const(i32::from(op.memory_bytes))
            .i32_const(alignment)
            .call(imports.cpu_read.expect("direct SIMD read"))
            .local_set(HELPER_STATUS);
        cpu_result::validate(code, u32::from(op.memory_bytes), 1, false, exit_depth);
    }
    let scalar_gpr = matches!(op.destination, SimdOperand::Register(SimdRegister::Gpr(_)));
    if !scalar_gpr {
        code.local_get(STATE_PTR);
    }
    match op.kind {
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
        SimdKind::Shuffle(kind) if matches!(kind, S::PackSigned | S::PackUnsigned) => {
            saturating_pack(code, op, kind);
        }
        SimdKind::Shuffle(kind)
            if matches!(
                kind,
                S::MaskBytes | S::MaskSingles | S::MaskDoubles | S::ExtractWord
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
                S::ExtractWord => {
                    code.i16x8_extract_lane_u(op.immediate.unwrap() % (op.vector_bytes / 2));
                }
                _ => unreachable!(),
            }
        }
        SimdKind::Shuffle(kind) if matches!(kind, S::ShiftBytesLeft | S::ShiftBytesRight) => {
            byte_shift(code, op, kind);
        }
        SimdKind::Shuffle(S::InsertWord) => insert_word(code, op),
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
