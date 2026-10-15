use crate::cpu::dbt::wasm::{helpers::protocol, imports::Imports};
use wasm_encoder::{BlockType, InstructionSink, MemArg};
mod access;
mod context;
pub(in crate::cpu::dbt::wasm) use access::{build_function, check_span};
pub(in crate::cpu::dbt::wasm) use context::{base, preflight};

use crate::{
    abi::{arena::SEGMENTS_OFFSET, x86::SEGMENTS_SELECTORS_OFFSET},
    cpu::{
        dbt::wasm::{
            instructions::memory,
            locals::{
                ADDRESS, BUDGET, DETAIL, EIP, FAULT_ACCESS, FAULT_ADDRESS, FAULT_LENGTH,
                HELPER_STATUS, REASON, RESULT, RETIRED, STATE_PTR, register,
            },
        },
        x86::{
            Register32, SegmentRegister,
            decode::DecodedOperation,
            ir::{Location32, Operation, SmallWidth, Value32},
        },
    },
};
pub(in crate::cpu::dbt::wasm) fn instruction(
    code: &mut InstructionSink<'_>,
    instruction: &DecodedOperation,
    imports: Imports,
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
            crate::cpu::dbt::wasm::instructions::stack::pointer::push_result(
                code, width, imports, exit_depth,
            );
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
                crate::cpu::dbt::wasm::abi::inhibit_debug_after_ss_load(code);
            }
            false
        }
        Operation::PopSegment { segment, word } => {
            crate::cpu::dbt::wasm::instructions::stack::pointer::load_result(
                code,
                if word { 2 } else { 4 },
                imports,
                exit_depth,
            );
            // POP SS advances using the old stack width before publishing the newly loaded cache.
            crate::cpu::dbt::wasm::instructions::stack::pointer::assigned(
                code,
                register(Register32::Esp),
                if word { 2 } else { 4 },
            );
            load_selector(code, segment, imports, exit_depth);
            code.local_set(register(Register32::Esp));
            if segment == SegmentRegister::Ss {
                crate::cpu::dbt::wasm::abi::inhibit_debug_after_ss_load(code);
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
            crate::cpu::dbt::wasm::helpers::result::validate(
                code,
                if word { 4 } else { 6 },
                1,
                false,
                exit_depth,
                crate::cpu::dbt::wasm::helpers::result::ResultContext::from(imports),
            );
            crate::cpu::dbt::wasm::helpers::result::value(code);
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
        protocol::exit_if_invalidated(code, exit_depth, imports);
    }
}

fn load_selector(
    code: &mut InstructionSink<'_>,
    segment: SegmentRegister,
    imports: Imports,
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
    crate::cpu::dbt::wasm::helpers::result::validate(
        code,
        0,
        0,
        false,
        exit_depth,
        crate::cpu::dbt::wasm::helpers::result::ResultContext::from(imports),
    );
}

pub(in crate::cpu::dbt::wasm) fn selector(
    code: &mut InstructionSink<'_>,
    segment: SegmentRegister,
) {
    code.local_get(STATE_PTR)
        .i32_load16_u(memarg(SEGMENTS_SELECTORS_OFFSET + segment.index() * 2, 1));
}

/// a null data selector may be loaded, but cannot be used for a 32-bit protected-mode memory reference.
pub(in crate::cpu::dbt::wasm) fn access(
    code: &mut InstructionSink<'_>,
    segment: SegmentRegister,
    exit_depth: u32,
) {
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
