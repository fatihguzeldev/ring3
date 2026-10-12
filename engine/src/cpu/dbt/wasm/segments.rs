use wasm_encoder::{BlockType, InstructionSink, MemArg};

use super::{
    locals::{
        ADDRESS, BUDGET, CANCEL_PTR, DETAIL, EIP, EXIT_PTR, FAULT_ACCESS, FAULT_ADDRESS,
        FAULT_LENGTH, HELPER_STATUS, MEMORY_BYTES, REASON, RESULT, RETIRED, STATE_PTR, register,
    },
    memory,
};
use crate::{
    abi::{
        ABI_VERSION, X86_INTEGER_PROFILE,
        arena::SEGMENTS_OFFSET,
        x86::{
            EXIT_SIZE, SEGMENTS_BASES_OFFSET, SEGMENTS_CACHE_OFFSETS, SEGMENTS_SELECTORS_OFFSET,
            SEGMENTS_SIZE, SEGMENTS_VERSION_2, STATE_SIZE,
        },
    },
    cpu::x86::{
        Register32, SegmentRegister,
        decode::DecodedInstruction,
        ir::{EffectiveAddress, Location32, Operation, SmallWidth, Value32},
    },
};

pub(super) fn instruction(
    code: &mut InstructionSink<'_>,
    instruction: &DecodedInstruction,
    imports: memory::Imports,
    exit_depth: u32,
) {
    let store = match *instruction.operation() {
        Operation::ReadSegment {
            segment,
            destination,
            word,
        } => {
            selector(code, segment);
            code.local_set(RESULT);
            match destination {
                Location32::Register(destination) => {
                    if word {
                        code.local_get(register(destination))
                            .i32_const(!0xffff)
                            .i32_and()
                            .local_get(RESULT)
                            .i32_or()
                            .local_set(register(destination));
                    } else {
                        code.local_get(RESULT).local_set(register(destination));
                    }
                    false
                }
                Location32::Memory(address) => {
                    memory::store_word_result(code, address, imports, exit_depth);
                    true
                }
            }
        }
        Operation::PushSegment { segment, word } => {
            selector(code, segment);
            code.local_set(RESULT);
            let width = if word { 2 } else { 4 };
            let address = EffectiveAddress {
                base: Some(Register32::Esp),
                displacement: 0_u32.wrapping_sub(width),
                ..Default::default()
            };
            if word {
                memory::store_word_result(code, address, imports, exit_depth);
            } else {
                memory::store_result(code, address, imports, exit_depth);
            }
            code.local_get(register(Register32::Esp))
                .i32_const(width as i32)
                .i32_sub()
                .local_set(register(Register32::Esp));
            true
        }
        Operation::LoadSegment { segment, source } => {
            match source {
                Value32::Register(source) => {
                    code.local_get(register(source)).local_set(RESULT);
                }
                Value32::Memory(address) => {
                    memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
                    code.local_set(RESULT);
                }
                Value32::Immediate(_) => unreachable!("segment loads have no immediate encoding"),
            }
            load_selector(code, segment, imports, exit_depth);
            if segment == SegmentRegister::Ss {
                super::abi::inhibit_debug_after_ss_load(code);
            }
            false
        }
        Operation::PopSegment { segment, word } => {
            let address = EffectiveAddress {
                base: Some(Register32::Esp),
                ..Default::default()
            };
            if word {
                memory::load_narrow_value(code, address, SmallWidth::Word, imports, exit_depth);
                code.local_set(RESULT);
            } else {
                memory::load_result(code, address, imports, exit_depth);
            }
            load_selector(code, segment, imports, exit_depth);
            code.local_get(register(Register32::Esp))
                .i32_const(if word { 2 } else { 4 })
                .i32_add()
                .local_set(register(Register32::Esp));
            if segment == SegmentRegister::Ss {
                super::abi::inhibit_debug_after_ss_load(code);
            }
            false
        }
        Operation::LoadFarPointer {
            segment,
            destination,
            address,
            word,
        } => {
            memory::address_value(code, address);
            code.local_set(ADDRESS);
            let index = imports
                .segment_load_far
                .expect("far pointer operation has a helper")
                .emit_binding(code);
            code.i32_const(segment.index() as i32)
                .local_get(ADDRESS)
                .i32_const(i32::from(word))
                .call(index)
                .local_set(HELPER_STATUS);
            super::cpu_result::validate(code, if word { 4 } else { 6 }, 1, false, exit_depth);
            super::cpu_result::value(code);
            code.local_set(RESULT);
            if word {
                code.local_get(register(destination))
                    .i32_const(!0xffff)
                    .i32_and()
                    .local_get(RESULT)
                    .i32_const(0xffff)
                    .i32_and()
                    .i32_or()
                    .local_set(register(destination));
            } else {
                code.local_get(RESULT).local_set(register(destination));
            }
            false
        }
        _ => unreachable!("segment emitter requires a segment operation"),
    };
    code.i32_const(instruction.next_pc().0 as i32)
        .local_set(EIP)
        .local_get(BUDGET)
        .i32_const(1)
        .i32_sub()
        .local_set(BUDGET)
        .local_get(RETIRED)
        .i32_const(1)
        .i32_add()
        .local_set(RETIRED);
    if store {
        memory::exit_if_invalidated(code, exit_depth);
    }
}

fn load_selector(
    code: &mut InstructionSink<'_>,
    segment: SegmentRegister,
    imports: memory::Imports,
    exit_depth: u32,
) {
    let index = imports
        .segment_load
        .expect("segment load has a helper")
        .emit_binding(code);
    code.i32_const(segment.index() as i32)
        .local_get(RESULT)
        .i32_const(0xffff)
        .i32_and()
        .call(index)
        .local_set(HELPER_STATUS);
    super::cpu_result::validate(code, 0, 0, false, exit_depth);
}

pub(super) fn preflight(code: &mut InstructionSink<'_>) {
    end(code);
    code.local_get(MEMORY_BYTES).i64_gt_u();
    fail_if(code, 1);
    for (pointer, size) in [
        (STATE_PTR, STATE_SIZE),
        (EXIT_PTR, EXIT_SIZE),
        (CANCEL_PTR, 4),
    ] {
        start(code);
        code.local_get(pointer)
            .i64_extend_i32_u()
            .i64_const(size as i64)
            .i64_add()
            .i64_lt_u()
            .local_get(pointer)
            .i64_extend_i32_u();
        end(code);
        code.i64_lt_u().i32_and();
        fail_if(code, 1);
    }
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
    fail_if(code, 2);
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

/// legacy segment bases are implied zero in both layouts; v2 reuses their storage for cache words.
pub(super) fn base(code: &mut InstructionSink<'_>, segment: SegmentRegister) {
    if matches!(segment, SegmentRegister::Fs | SegmentRegister::Gs) {
        code.local_get(STATE_PTR)
            .i32_load(memarg(SEGMENTS_BASES_OFFSET + segment.index() * 4, 2));
    } else {
        code.i32_const(0);
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

pub(super) fn selector(code: &mut InstructionSink<'_>, segment: SegmentRegister) {
    code.local_get(STATE_PTR)
        .i32_load16_u(memarg(SEGMENTS_SELECTORS_OFFSET + segment.index() * 2, 1));
}

/// a null data selector may be loaded, but cannot be used for a 32-bit protected-mode memory reference.
pub(super) fn access(code: &mut InstructionSink<'_>, segment: SegmentRegister, exit_depth: u32) {
    selector(code, segment);
    code.i32_const(0xfffc)
        .i32_and()
        .i32_eqz()
        .if_(BlockType::Empty)
        .i32_const(11)
        .local_set(REASON)
        .i32_const(0)
        .local_set(DETAIL)
        .i32_const(0)
        .local_set(FAULT_ADDRESS)
        .i32_const(0)
        .local_set(FAULT_ACCESS)
        .i32_const(0)
        .local_set(FAULT_LENGTH)
        .br(exit_depth + 1)
        .end();
}

fn start(code: &mut InstructionSink<'_>) {
    code.local_get(STATE_PTR)
        .i64_extend_i32_u()
        .i64_const(SEGMENTS_OFFSET as i64)
        .i64_add();
}

fn end(code: &mut InstructionSink<'_>) {
    start(code);
    code.i64_const(SEGMENTS_SIZE as i64).i64_add();
}

fn memarg(offset: usize, align: u32) -> MemArg {
    MemArg {
        offset: (SEGMENTS_OFFSET + offset) as u64,
        align,
        memory_index: 0,
    }
}

fn fail_if(code: &mut InstructionSink<'_>, status: i32) {
    code.if_(BlockType::Empty).i32_const(status).return_().end();
}
