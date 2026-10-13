use wasm_encoder::{BlockType, InstructionSink, MemArg};

mod access;
mod context;
pub(super) use access::{build_function, check_span};
pub(super) use context::{base, preflight};

use super::{
    locals::{
        ADDRESS, BUDGET, DETAIL, EIP, FAULT_ACCESS, FAULT_ADDRESS, FAULT_LENGTH, HELPER_STATUS,
        REASON, RESULT, RETIRED, STATE_PTR, register,
    },
    memory,
};
use crate::{
    abi::{arena::SEGMENTS_OFFSET, x86::SEGMENTS_SELECTORS_OFFSET},
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
            memory::checked_address(
                code,
                address,
                if word { 4 } else { 6 },
                crate::cpu::x86::SegmentAccess::Read,
                imports,
                exit_depth,
            );
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

fn memarg(offset: usize, align: u32) -> MemArg {
    MemArg {
        offset: (SEGMENTS_OFFSET + offset) as u64,
        align,
        memory_index: 0,
    }
}
