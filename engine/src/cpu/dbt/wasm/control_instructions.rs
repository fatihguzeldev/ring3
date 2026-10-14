use wasm_encoder::{BlockType, InstructionSink, MemArg};

use super::{cpu_result, locals::*, memory};
use crate::{
    abi::arena::TRANSFER_OFFSET,
    cpu::x86::{
        ControlOperation, Register32, USER_POP_FLAGS_MASK, USER_POP_FLAGS_WORD_MASK,
        decode::DecodedInstruction,
        ir::{AddressSize, EffectiveAddress, Operation},
    },
};

pub(super) fn instruction(
    code: &mut InstructionSink<'_>,
    instruction: &DecodedInstruction,
    imports: memory::Imports,
    exit_depth: u32,
) {
    let Operation::Control(operation) = *instruction.operation() else {
        unreachable!("control emitter requires control operation")
    };
    match operation {
        ControlOperation::PopFlags { word } => pop_flags(code, word, imports, exit_depth),
        ControlOperation::InvalidOpcode => {
            exit(code, 20, exit_depth);
            return;
        }
        ControlOperation::DeviceNotAvailable => {
            exit(code, 22, exit_depth);
            return;
        }
        ControlOperation::GeneralProtectionZero => {
            exit(code, 11, exit_depth);
            return;
        }
        ControlOperation::DeniedPortString { repeat_count } => {
            if let Some(address_size) = repeat_count {
                code.local_get(register(Register32::Ecx));
                if address_size == AddressSize::Bits16 {
                    code.i32_const(0xffff).i32_and();
                }
                code.if_(BlockType::Empty);
                exit(code, 11, exit_depth + 1);
                code.end();
            } else {
                exit(code, 11, exit_depth);
                return;
            }
        }
        ControlOperation::Breakpoint => {
            retire(code, instruction.next_pc().0);
            exit(code, 17, exit_depth);
            return;
        }
        ControlOperation::DebugTrap => {
            super::detection::mark_debug(code, imports.detection, 1);
            retire(code, instruction.next_pc().0);
            exit(code, 16, exit_depth);
            return;
        }
        ControlOperation::Overflow => {
            code.local_get(FLAGS)
                .i32_const(0x800)
                .i32_and()
                .if_(BlockType::Empty);
            retire(code, instruction.next_pc().0);
            exit(code, 18, exit_depth + 1);
            code.end();
        }
        ControlOperation::Bound {
            index,
            address,
            word,
        } => {
            bound(code, index, address, word, imports, exit_depth);
        }
    }
    retire(code, instruction.next_pc().0);
}

fn pop_flags(
    code: &mut InstructionSink<'_>,
    word: bool,
    imports: memory::Imports,
    exit_depth: u32,
) {
    super::stack::pointer::load_result(code, if word { 2 } else { 4 }, imports, exit_depth);
    let mutable = if word {
        USER_POP_FLAGS_WORD_MASK
    } else {
        USER_POP_FLAGS_MASK
    };
    code.local_get(FLAGS)
        .i32_const((!mutable & !0x10000) as i32)
        .i32_and()
        .local_get(RESULT)
        .i32_const(mutable as i32)
        .i32_and()
        .i32_or()
        .local_set(FLAGS);
    super::stack::pointer::advance(
        code,
        if word { 2 } else { 4 },
        imports.raw_task_context.is_some(),
    );
}

fn bound(
    code: &mut InstructionSink<'_>,
    index: Register32,
    address: EffectiveAddress,
    word: bool,
    imports: memory::Imports,
    exit_depth: u32,
) {
    let width = if word { 2 } else { 4 };
    memory::checked_address(
        code,
        address,
        (width * 2) as u32,
        crate::cpu::x86::SegmentAccess::Read,
        imports,
        exit_depth,
    );
    code.local_get(ADDRESS)
        .i32_const(width * 2)
        .i32_const(1)
        .call(imports.cpu_read.expect("BOUND memory read import"))
        .local_set(HELPER_STATUS);
    crate::cpu::dbt::wasm::code_fetch::check_context(
        code,
        exit_depth,
        imports.raw_task_context.is_some(),
    );
    cpu_result::validate(
        code,
        (width * 2) as u32,
        1,
        false,
        exit_depth,
        imports.raw_task_context.is_some(),
    );
    code.local_get(register(index));
    if word {
        code.i32_extend16_s();
    }
    code.local_set(RESULT)
        .local_get(RESULT)
        .local_get(STATE_PTR);
    load_bound(code, 0, word);
    code.i32_lt_s().local_get(RESULT).local_get(STATE_PTR);
    load_bound(code, width as usize, word);
    code.i32_gt_s().i32_or().if_(BlockType::Empty);
    exit(code, 19, exit_depth + 1);
    code.end();
}

fn load_bound(code: &mut InstructionSink<'_>, offset: usize, word: bool) {
    let argument = MemArg {
        offset: (TRANSFER_OFFSET + offset) as u64,
        align: if word { 1 } else { 2 },
        memory_index: 0,
    };
    if word {
        code.i32_load16_s(argument);
    } else {
        code.i32_load(argument);
    }
}

fn retire(code: &mut InstructionSink<'_>, next_pc: u32) {
    code.i32_const(next_pc as i32)
        .local_set(EIP)
        .local_get(BUDGET)
        .i32_const(1)
        .i32_sub()
        .local_set(BUDGET)
        .local_get(RETIRED)
        .i32_const(1)
        .i32_add()
        .local_set(RETIRED);
}

fn exit(code: &mut InstructionSink<'_>, reason: i32, exit_depth: u32) {
    code.i32_const(reason).local_set(REASON);
    for local in [DETAIL, FAULT_ADDRESS, FAULT_ACCESS, FAULT_LENGTH] {
        code.i32_const(0).local_set(local);
    }
    code.br(exit_depth);
}
