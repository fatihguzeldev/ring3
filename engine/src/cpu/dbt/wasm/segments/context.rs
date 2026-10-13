use super::super::locals::{CANCEL_PTR, EXIT_PTR, MEMORY_BYTES, RESULT, STATE_PTR};
use super::{memarg, selector};
use crate::{
    abi::{
        ABI_VERSION, X86_INTEGER_PROFILE,
        arena::{SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET},
        x86::{
            EXIT_SIZE, SEGMENT_BASES_RECORD_SIZE, SEGMENTS_BASES_OFFSET, SEGMENTS_CACHE_OFFSETS,
            SEGMENTS_SELECTORS_OFFSET, SEGMENTS_SIZE, SEGMENTS_VERSION_2, SEGMENTS_VERSION_3,
            STATE_SIZE,
        },
    },
    cpu::x86::SegmentRegister,
};
use wasm_encoder::{BlockType, InstructionSink, MemArg, ValType};

pub(in crate::cpu::dbt::wasm) fn preflight(code: &mut InstructionSink<'_>) {
    range(code, SEGMENTS_OFFSET, SEGMENTS_SIZE);
    for (offset, expected) in [
        (0, u32::from_le_bytes(*b"R3SG")),
        (8, SEGMENTS_SIZE as u32),
        (12, 0),
        (60, 0),
    ] {
        code.local_get(STATE_PTR)
            .i32_load(memarg(offset, 2))
            .i32_const(expected as i32)
            .i32_ne();
        fail_if(code, 2);
    }
    let legacy_header = u32::from(ABI_VERSION) | (u32::from(X86_INTEGER_PROFILE) << 16);
    code.local_get(STATE_PTR)
        .i32_load(memarg(4, 2))
        .i32_const(legacy_header as i32)
        .i32_eq()
        .if_(BlockType::Empty);
    for offset in [28, 32, 36, 40, 44, 56] {
        code.local_get(STATE_PTR).i32_load(memarg(offset, 2));
        fail_if(code, 2);
    }
    code.else_();
    code.local_get(STATE_PTR)
        .i32_load(memarg(4, 2))
        .i32_const((u32::from(SEGMENTS_VERSION_2) | (u32::from(X86_INTEGER_PROFILE) << 16)) as i32)
        .i32_ne();
    code.local_get(STATE_PTR)
        .i32_load(memarg(4, 2))
        .i32_const((u32::from(SEGMENTS_VERSION_3) | (u32::from(X86_INTEGER_PROFILE) << 16)) as i32)
        .i32_ne()
        .i32_and();
    fail_if(code, 2);
    code.local_get(STATE_PTR)
        .i32_load16_u(memarg(4, 1))
        .i32_const(i32::from(SEGMENTS_VERSION_3))
        .i32_eq()
        .if_(BlockType::Empty);
    bases_preflight(code);
    code.end();
    for (index, offset) in SEGMENTS_CACHE_OFFSETS.into_iter().enumerate() {
        cache_preflight(code, index, offset);
    }
    code.end();
    for segment in [SegmentRegister::Cs, SegmentRegister::Ss] {
        selector(code, segment);
        code.i32_const(0xfffc).i32_and().i32_eqz();
        fail_if(code, 2);
        selector(code, segment);
        code.i32_const(3).i32_and().i32_const(3).i32_ne();
        fail_if(code, 2);
    }
}

pub(in crate::cpu::dbt::wasm) fn base(code: &mut InstructionSink<'_>, segment: SegmentRegister) {
    if segment == SegmentRegister::Cs {
        code.i32_const(0);
        return;
    }
    code.local_get(STATE_PTR)
        .i32_load16_u(memarg(4, 1))
        .i32_const(i32::from(SEGMENTS_VERSION_3))
        .i32_eq()
        .if_(BlockType::Result(ValType::I32));
    code.local_get(STATE_PTR)
        .i32_load(arg(SEGMENT_BASES_RECORD_OFFSET + 16 + segment.index() * 4));
    code.else_();
    if matches!(segment, SegmentRegister::Fs | SegmentRegister::Gs) {
        code.local_get(STATE_PTR)
            .i32_load(memarg(SEGMENTS_BASES_OFFSET + segment.index() * 4, 2));
    } else {
        code.i32_const(0);
    }
    code.end();
}

fn bases_preflight(code: &mut InstructionSink<'_>) {
    range(code, SEGMENT_BASES_RECORD_OFFSET, SEGMENT_BASES_RECORD_SIZE);
    for offset in [48, 52] {
        code.local_get(STATE_PTR).i32_load(memarg(offset, 2));
        fail_if(code, 2);
    }
    for (offset, expected) in [
        (0, u32::from_le_bytes(*b"R3SB")),
        (
            4,
            u32::from(ABI_VERSION) | (u32::from(X86_INTEGER_PROFILE) << 16),
        ),
        (8, SEGMENT_BASES_RECORD_SIZE as u32),
        (12, 0),
        (16 + 4 * SegmentRegister::Cs.index(), 0),
    ] {
        code.local_get(STATE_PTR)
            .i32_load(arg(SEGMENT_BASES_RECORD_OFFSET + offset))
            .i32_const(expected as i32)
            .i32_ne();
        fail_if(code, 2);
    }
}

fn cache_preflight(code: &mut InstructionSink<'_>, index: usize, offset: usize) {
    code.local_get(STATE_PTR)
        .i32_load(memarg(offset, 2))
        .local_set(RESULT);
    code.local_get(STATE_PTR)
        .i32_load16_u(memarg(SEGMENTS_SELECTORS_OFFSET + index * 2, 1))
        .i32_const(0xfffc)
        .i32_and()
        .i32_eqz()
        .if_(BlockType::Empty);
    code.local_get(RESULT);
    fail_if(code, 2);
    code.else_();
    // usable loaded caches are present code/data descriptors with L=0.
    cache_mask(code, 0x9400_0000, 0x9000_0000);
    match index {
        1 => {
            cache_mask(code, 0x0280_0000, 0x0280_0000);
            code.local_get(RESULT)
                .i32_const(0x0040_0000)
                .i32_and()
                .i32_eqz()
                .if_(BlockType::Empty);
            cache_mask(code, 0x6000_0000, 0x6000_0000);
            code.end();
        }
        2 => {
            cache_mask(code, 0x02a0_0000, 0x0220_0000);
            cache_mask(code, 0x6000_0000, 0x6000_0000);
        }
        _ => {
            code.local_get(RESULT)
                .i32_const(0x00a0_0000)
                .i32_and()
                .i32_const(0x0080_0000)
                .i32_eq();
            fail_if(code, 2);
            code.local_get(RESULT)
                .i32_const(0x00c0_0000)
                .i32_and()
                .i32_const(0x00c0_0000)
                .i32_ne()
                .if_(BlockType::Empty);
            cache_mask(code, 0x6000_0000, 0x6000_0000);
            code.end();
        }
    }
    code.end();
}

fn cache_mask(code: &mut InstructionSink<'_>, mask: u32, expected: u32) {
    code.local_get(RESULT)
        .i32_const(mask as i32)
        .i32_and()
        .i32_const(expected as i32)
        .i32_ne();
    fail_if(code, 2);
}

fn range(code: &mut InstructionSink<'_>, offset: usize, size: usize) {
    range_start(code, offset);
    code.i64_const(size as i64)
        .i64_add()
        .local_get(MEMORY_BYTES)
        .i64_gt_u();
    fail_if(code, 1);
    for (pointer, length) in [
        (STATE_PTR, STATE_SIZE),
        (EXIT_PTR, EXIT_SIZE),
        (CANCEL_PTR, 4),
    ] {
        range_start(code, offset);
        code.local_get(pointer)
            .i64_extend_i32_u()
            .i64_const(length as i64)
            .i64_add()
            .i64_lt_u()
            .local_get(pointer)
            .i64_extend_i32_u();
        range_start(code, offset);
        code.i64_const(size as i64).i64_add().i64_lt_u().i32_and();
        fail_if(code, 1);
    }
}

fn range_start(code: &mut InstructionSink<'_>, offset: usize) {
    code.local_get(STATE_PTR)
        .i64_extend_i32_u()
        .i64_const(offset as i64)
        .i64_add();
}

fn arg(offset: usize) -> MemArg {
    MemArg {
        offset: offset as u64,
        align: 2,
        memory_index: 0,
    }
}

fn fail_if(code: &mut InstructionSink<'_>, status: i32) {
    code.if_(BlockType::Empty).i32_const(status).return_().end();
}
