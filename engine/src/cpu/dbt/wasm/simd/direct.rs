use super::super::{cpu_result, locals::*};
use super::{Imports, SimdKind, SimdOperand, SimdOperation, SimdRegister, mark_mmx, memory};
use crate::{
    abi::{
        arena::{SIMD_OFFSET, TRANSFER_OFFSET, X87_OFFSET},
        x86::{SIMD_XMM_OFFSET, X87_REGISTERS_OFFSET},
    },
    cpu::x86::simd::{PackedKind as P, ShuffleKind as S},
};
use wasm_encoder::{InstructionSink, MemArg};

pub(super) fn accepts(op: SimdOperation) -> bool {
    match op.kind {
        SimdKind::Move => true,
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
                | P::MultiplyAddWords
                | P::AverageUnsigned
                | P::MinimumSigned
                | P::MaximumSigned
                | P::MinimumUnsigned
                | P::MaximumUnsigned
        ),
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
                | S::MoveLowToHigh
                | S::MoveHighToLow
                | S::MaskBytes
                | S::MaskSingles
                | S::MaskDoubles
                | S::ExtractWord
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
        (P::GreaterSigned, 1) => {
            code.i8x16_gt_s();
        }
        (P::GreaterSigned, 2) => {
            code.i16x8_gt_s();
        }
        (P::GreaterSigned, 4) => {
            code.i32x4_gt_s();
        }
        (P::MultiplyLow, 2) => {
            code.i16x8_mul();
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
        _ => unreachable!("direct packed opcode and width were decoded"),
    }
}

fn shuffle(code: &mut InstructionSink<'_>, op: SimdOperation, kind: S) {
    let width = usize::from(op.lane_bytes);
    let size = usize::from(op.vector_bytes);
    let immediate = op.immediate.unwrap_or(0);
    let source_only = matches!(kind, S::Dwords | S::Words | S::HighWords | S::LowWords);
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
        SimdKind::Packed(kind) => {
            load(code, op.destination, op.vector_bytes);
            if kind == P::AndNot {
                code.v128_not();
            }
            load(code, op.source, op.vector_bytes);
            arithmetic(code, kind, op.lane_bytes);
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
            code.v128_store(arg(TRANSFER_OFFSET));
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
