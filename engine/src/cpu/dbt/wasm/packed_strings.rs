use super::{cpu_result, locals::*, memory};
use crate::{
    abi::{
        arena::{SIMD_OFFSET, TRANSFER_OFFSET},
        x86::SIMD_XMM_OFFSET,
    },
    cpu::x86::{
        Register32,
        packed_strings::{
            PackedStringCompare, PackedStringLength, PackedStringOutput, PackedStringSource,
        },
    },
};
use wasm_encoder::{BlockType, InstructionSink, MemArg, ValType};

fn arg(offset: usize) -> MemArg {
    MemArg {
        offset: offset as u64,
        align: 0,
        memory_index: 0,
    }
}

fn load(code: &mut InstructionSink<'_>, source: PackedStringSource) {
    let offset = match source {
        PackedStringSource::Xmm(index) => SIMD_OFFSET + SIMD_XMM_OFFSET + usize::from(index) * 16,
        PackedStringSource::Memory(_) => TRANSFER_OFFSET,
    };
    code.local_get(STATE_PTR).v128_load(arg(offset));
}

fn bitmask(code: &mut InstructionSink<'_>, words: bool) {
    if words {
        code.i16x8_bitmask();
    } else {
        code.i8x16_bitmask();
    }
}

fn equal(code: &mut InstructionSink<'_>, words: bool) {
    if words {
        code.i16x8_eq();
    } else {
        code.i8x16_eq();
    }
}

fn splat_left(code: &mut InstructionSink<'_>, left: u8, lane: usize, words: bool) {
    let offset = SIMD_OFFSET + SIMD_XMM_OFFSET + usize::from(left) * 16;
    code.local_get(STATE_PTR);
    if words {
        code.i32_load16_u(arg(offset + lane * 2)).i16x8_splat();
    } else {
        code.i32_load8_u(arg(offset + lane)).i8x16_splat();
    }
}

fn effective_length(
    code: &mut InstructionSink<'_>,
    source: PackedStringSource,
    mode: PackedStringLength,
    explicit: Register32,
    words: bool,
    count: i32,
    destination: u32,
) {
    match mode {
        PackedStringLength::Explicit => {
            // unsigned saturation also handles the absolute value of i32::MIN.
            code.local_get(register(explicit))
                .local_get(register(explicit))
                .i32_const(31)
                .i32_shr_s()
                .i32_xor()
                .local_get(register(explicit))
                .i32_const(31)
                .i32_shr_s()
                .i32_sub()
                .local_tee(destination)
                .i32_const(count)
                .i32_lt_u()
                .if_(BlockType::Result(ValType::I32))
                .local_get(destination)
                .else_()
                .i32_const(count)
                .end()
                .local_set(destination);
        }
        PackedStringLength::NullTerminated => {
            load(code, source);
            code.v128_const(0);
            equal(code, words);
            bitmask(code, words);
            code.i32_const(1 << count)
                .i32_or()
                .i32_ctz()
                .local_set(destination);
        }
    }
}

fn equal_any(code: &mut InstructionSink<'_>, op: PackedStringCompare, words: bool, count: usize) {
    for lane in 0..count {
        code.local_get(LHS)
            .i32_const(lane as i32)
            .i32_gt_u()
            .if_(BlockType::Empty);
        code.local_get(RESULT);
        splat_left(code, op.left, lane, words);
        load(code, op.right);
        equal(code, words);
        bitmask(code, words);
        code.i32_or().local_set(RESULT).end();
    }
    code.local_get(RESULT)
        .local_get(ADDRESS)
        .i32_and()
        .local_set(RESULT);
}

fn ranges(code: &mut InstructionSink<'_>, op: PackedStringCompare, words: bool, count: usize) {
    let signed = op.control & 2 != 0;
    for lane in (0..count).step_by(2) {
        code.local_get(LHS)
            .i32_const((lane + 1) as i32)
            .i32_gt_u()
            .if_(BlockType::Empty);
        code.local_get(RESULT);
        load(code, op.right);
        splat_left(code, op.left, lane, words);
        match (words, signed) {
            (false, false) => {
                code.i8x16_ge_u();
            }
            (false, true) => {
                code.i8x16_ge_s();
            }
            (true, false) => {
                code.i16x8_ge_u();
            }
            (true, true) => {
                code.i16x8_ge_s();
            }
        }
        load(code, op.right);
        splat_left(code, op.left, lane + 1, words);
        match (words, signed) {
            (false, false) => {
                code.i8x16_le_u();
            }
            (false, true) => {
                code.i8x16_le_s();
            }
            (true, false) => {
                code.i16x8_le_u();
            }
            (true, true) => {
                code.i16x8_le_s();
            }
        }
        code.v128_and();
        bitmask(code, words);
        code.i32_or().local_set(RESULT).end();
    }
    code.local_get(RESULT)
        .local_get(ADDRESS)
        .i32_and()
        .local_set(RESULT);
}

fn equal_each(
    code: &mut InstructionSink<'_>,
    op: PackedStringCompare,
    words: bool,
    full_mask: i32,
) {
    load(code, PackedStringSource::Xmm(op.left));
    load(code, op.right);
    equal(code, words);
    bitmask(code, words);
    code.local_get(RESUME)
        .i32_and()
        .local_get(ADDRESS)
        .i32_and()
        .local_get(RESUME)
        .local_get(ADDRESS)
        .i32_or()
        .i32_const(-1)
        .i32_xor()
        .i32_or()
        .i32_const(full_mask)
        .i32_and()
        .local_set(RESULT);
}

fn equal_ordered(
    code: &mut InstructionSink<'_>,
    op: PackedStringCompare,
    words: bool,
    count: usize,
) {
    let width = if words { 2 } else { 1 };
    for start in 0..count {
        code.local_get(RESULT).local_get(RESUME);
        load(code, PackedStringSource::Xmm(op.left));
        load(code, op.right);
        if start != 0 {
            code.v128_const(0)
                .i8x16_shuffle(std::array::from_fn(|index| {
                    let shifted = start * width + index;
                    if shifted < 16 { shifted as u8 } else { 16 }
                }));
        }
        equal(code, words);
        bitmask(code, words);
        // invalid left elements match; valid left elements require a valid right
        // element and equality. comparisons stop at the physical vector end.
        code.local_get(ADDRESS)
            .i32_const(start as i32)
            .i32_shr_u()
            .i32_and()
            .i32_const(-1)
            .i32_xor()
            .i32_and()
            .i32_const(((1u32 << (count - start)) - 1) as i32)
            .i32_and()
            .i32_eqz()
            .i32_const(start as i32)
            .i32_shl()
            .i32_or()
            .local_set(RESULT);
    }
}

fn publish(code: &mut InstructionSink<'_>, op: PackedStringCompare, words: bool, count: usize) {
    code.local_get(FLAGS)
        .i32_const(!0x8d5)
        .i32_and()
        .local_get(RESULT)
        .i32_eqz()
        .i32_eqz()
        .i32_or()
        .local_get(RESULT)
        .i32_const(1)
        .i32_and()
        .i32_const(11)
        .i32_shl()
        .i32_or()
        .local_get(LHS)
        .i32_const(count as i32)
        .i32_lt_u()
        .i32_const(7)
        .i32_shl()
        .i32_or()
        .local_get(RHS)
        .i32_const(count as i32)
        .i32_lt_u()
        .i32_const(6)
        .i32_shl()
        .i32_or()
        .local_set(FLAGS);
    match op.output {
        PackedStringOutput::Index => {
            code.local_get(RESULT).if_(BlockType::Result(ValType::I32));
            if op.control & 0x40 == 0 {
                code.local_get(RESULT).i32_ctz();
            } else {
                code.i32_const(31).local_get(RESULT).i32_clz().i32_sub();
            }
            code.else_()
                .i32_const(count as i32)
                .end()
                .local_set(register(Register32::Ecx));
        }
        PackedStringOutput::Mask => {
            code.local_get(STATE_PTR).v128_const(0);
            if op.control & 0x40 == 0 {
                code.local_get(RESULT).i32x4_replace_lane(0);
            } else {
                for lane in 0..count {
                    code.i32_const(0)
                        .local_get(RESULT)
                        .i32_const(lane as i32)
                        .i32_shr_u()
                        .i32_const(1)
                        .i32_and()
                        .i32_sub();
                    if words {
                        code.i16x8_replace_lane(lane as u8);
                    } else {
                        code.i8x16_replace_lane(lane as u8);
                    }
                }
            }
            code.v128_store(arg(SIMD_OFFSET + SIMD_XMM_OFFSET));
        }
    }
}

pub(super) fn instruction(
    code: &mut InstructionSink<'_>,
    op: PackedStringCompare,
    imports: memory::Imports,
    exit_depth: u32,
) -> bool {
    if let PackedStringSource::Memory(address) = op.right {
        memory::checked_address(
            code,
            address,
            16,
            crate::cpu::x86::SegmentAccess::Read,
            imports,
            exit_depth,
        );
        code.local_get(ADDRESS)
            .i32_const(16)
            .i32_const(1)
            .call(imports.cpu_read.expect("packed-string read import"))
            .local_set(HELPER_STATUS);
        cpu_result::validate(code, 16, 1, false, exit_depth);
    }
    let words = op.control & 1 != 0;
    let count = if words { 8 } else { 16 };
    effective_length(
        code,
        PackedStringSource::Xmm(op.left),
        op.length,
        Register32::Eax,
        words,
        count,
        LHS,
    );
    effective_length(
        code,
        op.right,
        op.length,
        Register32::Edx,
        words,
        count,
        RHS,
    );
    code.i32_const(1)
        .local_get(LHS)
        .i32_shl()
        .i32_const(1)
        .i32_sub()
        .local_set(RESUME)
        .i32_const(1)
        .local_get(RHS)
        .i32_shl()
        .i32_const(1)
        .i32_sub()
        .local_set(ADDRESS)
        .i32_const(0)
        .local_set(RESULT);
    let count = count as usize;
    let full_mask = ((1u32 << count) - 1) as i32;
    match op.control >> 2 & 3 {
        0 => equal_any(code, op, words, count),
        1 => ranges(code, op, words, count),
        2 => equal_each(code, op, words, full_mask),
        3 => equal_ordered(code, op, words, count),
        _ => unreachable!(),
    }
    match op.control >> 4 & 3 {
        1 => {
            code.local_get(RESULT)
                .i32_const(full_mask)
                .i32_xor()
                .local_set(RESULT);
        }
        3 => {
            code.local_get(RESULT)
                .local_get(ADDRESS)
                .i32_xor()
                .local_set(RESULT);
        }
        _ => {}
    }
    publish(code, op, words, count);
    false
}
