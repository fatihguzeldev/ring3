use wasm_encoder::{BlockType, InstructionSink};

use super::{abi, locals::*, memory};
use crate::cpu::x86::{
    Register32, SegmentRegister,
    decode::DecodedInstruction,
    ir::{
        AddressSize, EffectiveAddress, Operation, RepeatCondition, SmallWidth, StringKind,
        StringWidth,
    },
};

pub(super) fn instruction(
    code: &mut InstructionSink<'_>,
    instruction: &DecodedInstruction,
    imports: memory::Imports,
    exit_depth: u32,
) {
    let (kind, width, repeat, address_size, source_segment) = match *instruction.operation() {
        Operation::RepeatString {
            kind,
            width,
            repeat,
            address_size,
            source_segment,
        } => (kind, width, Some(repeat), address_size, source_segment),
        Operation::String {
            kind,
            width,
            address_size,
            source_segment,
        } => (kind, width, None, address_size, source_segment),
        _ => unreachable!("string emitter requires string operation"),
    };
    let source = EffectiveAddress {
        base: Some(Register32::Esi),
        index: None,
        scale: 1,
        displacement: 0,
        segment: source_segment,
        address_size,
    };
    // a string destination is always ES, regardless of the source segment override.
    let destination = EffectiveAddress {
        base: Some(Register32::Edi),
        index: None,
        scale: 1,
        displacement: 0,
        segment: Some(SegmentRegister::Es),
        address_size,
    };
    let stores = matches!(kind, StringKind::Move | StringKind::Store);
    if stores {
        code.i32_const(0).local_set(HELPER_STATUS);
    }
    let Some(repeat) = repeat else {
        element(
            code,
            kind,
            width,
            address_size,
            source,
            destination,
            imports,
            exit_depth,
        );
        charge(code);
        complete(code, instruction);
        if stores {
            memory::exit_if_invalidated(code, exit_depth, imports);
        }
        return;
    };
    code.block(BlockType::Empty).loop_(BlockType::Empty);
    count(code, address_size);
    code.i32_eqz().if_(BlockType::Empty);
    // the zero-count instruction completes without touching either guest address.
    charge(code);
    complete(code, instruction);
    abi::debug_after_retirement(code, exit_depth + 3, imports.detection, false);
    code.br(2).end();
    abi::safepoint(code, exit_depth + 2);
    element(
        code,
        kind,
        width,
        address_size,
        source,
        destination,
        imports,
        exit_depth + 2,
    );
    decrement_count(code, address_size);
    charge(code);
    count(code, address_size);
    code.i32_eqz();
    match repeat {
        RepeatCondition::Always => {}
        RepeatCondition::Equal => {
            code.local_get(FLAGS)
                .i32_const(0x40)
                .i32_and()
                .i32_eqz()
                .i32_or();
        }
        RepeatCondition::NotEqual => {
            code.local_get(FLAGS)
                .i32_const(0x40)
                .i32_and()
                .i32_eqz()
                .i32_eqz()
                .i32_or();
        }
    }
    code.if_(BlockType::Empty);
    complete(code, instruction);
    abi::debug_after_retirement(code, exit_depth + 3, imports.detection, false);
    code.br(2).end();
    // single-step an unfinished repetition with EIP still at the string instruction.
    abi::debug_after_retirement(code, exit_depth + 2, imports.detection, true);
    if stores {
        // a committed executable write stops repetition at the following element.
        memory::exit_if_invalidated(code, exit_depth + 2, imports);
    }
    code.br(0).end().end();
    if stores {
        memory::exit_if_invalidated(code, exit_depth, imports);
    }
    // re-enter the ordinary dispatcher even on completion; it owns the next PC.
    code.br(exit_depth - 1);
}

#[allow(clippy::too_many_arguments)]
fn element(
    code: &mut InstructionSink<'_>,
    kind: StringKind,
    width: StringWidth,
    address_size: AddressSize,
    source: EffectiveAddress,
    destination: EffectiveAddress,
    imports: memory::Imports,
    exit_depth: u32,
) {
    let bytes = match width {
        StringWidth::Byte => 1,
        StringWidth::Word => 2,
        StringWidth::Dword => 4,
    };
    if matches!(
        kind,
        StringKind::Move | StringKind::Load | StringKind::Compare
    ) {
        memory::checked_address(
            code,
            source,
            bytes,
            crate::cpu::x86::SegmentAccess::Read,
            imports,
            exit_depth,
        );
    }
    if matches!(
        kind,
        StringKind::Move | StringKind::Store | StringKind::Compare | StringKind::Scan
    ) {
        memory::checked_address(
            code,
            destination,
            bytes,
            if matches!(kind, StringKind::Move | StringKind::Store) {
                crate::cpu::x86::SegmentAccess::Write
            } else {
                crate::cpu::x86::SegmentAccess::Read
            },
            imports,
            exit_depth,
        );
    }
    match kind {
        StringKind::Move => {
            read(code, source, width, imports, exit_depth);
            code.local_set(RESULT);
            write(code, destination, width, imports, exit_depth);
        }
        StringKind::Store => {
            accumulator(code, width);
            code.local_set(RESULT);
            write(code, destination, width, imports, exit_depth);
        }
        StringKind::Load => {
            read(code, source, width, imports, exit_depth);
            code.local_set(RESULT);
            let mask = width_mask(width);
            if mask != -1 {
                code.local_get(register(Register32::Eax))
                    .i32_const(!mask)
                    .i32_and();
                code.local_get(RESULT).i32_or();
            } else {
                code.local_get(RESULT);
            }
            code.local_set(register(Register32::Eax));
        }
        StringKind::Compare => {
            // retain the first value on the Wasm operand stack across the second helper.
            read(code, source, width, imports, exit_depth);
            read(code, destination, width, imports, exit_depth);
            code.local_set(RHS).local_set(LHS);
            compare_flags(code, width);
        }
        StringKind::Scan => {
            read(code, destination, width, imports, exit_depth);
            code.local_set(RHS);
            accumulator(code, width);
            code.local_set(LHS);
            compare_flags(code, width);
        }
    }
    if matches!(
        kind,
        StringKind::Move | StringKind::Load | StringKind::Compare
    ) {
        advance(code, Register32::Esi, width, address_size);
    }
    if matches!(
        kind,
        StringKind::Move | StringKind::Store | StringKind::Compare | StringKind::Scan
    ) {
        advance(code, Register32::Edi, width, address_size);
    }
}

fn complete(code: &mut InstructionSink<'_>, instruction: &DecodedInstruction) {
    code.i32_const(instruction.next_pc().0 as i32)
        .local_set(EIP)
        .local_get(RETIRED)
        .i32_const(1)
        .i32_add()
        .local_set(RETIRED);
}

fn charge(code: &mut InstructionSink<'_>) {
    code.local_get(BUDGET)
        .i32_const(1)
        .i32_sub()
        .local_set(BUDGET);
}

fn count(code: &mut InstructionSink<'_>, address_size: AddressSize) {
    code.local_get(register(Register32::Ecx));
    if address_size == AddressSize::Bits16 {
        code.i32_const(0xffff).i32_and();
    }
}

fn decrement_count(code: &mut InstructionSink<'_>, address_size: AddressSize) {
    count(code, address_size);
    code.i32_const(1).i32_sub();
    if address_size == AddressSize::Bits16 {
        code.i32_const(0xffff)
            .i32_and()
            .local_get(register(Register32::Ecx))
            .i32_const(!0xffff)
            .i32_and()
            .i32_or();
    }
    code.local_set(register(Register32::Ecx));
}

fn advance(
    code: &mut InstructionSink<'_>,
    pointer: Register32,
    width: StringWidth,
    address_size: AddressSize,
) {
    let bytes = match width {
        StringWidth::Byte => 1,
        StringWidth::Word => 2,
        StringWidth::Dword => 4,
    };
    code.local_get(register(pointer))
        .i32_const(-bytes)
        .i32_const(bytes)
        .local_get(FLAGS)
        .i32_const(0x400)
        .i32_and()
        .select()
        .i32_add();
    if address_size == AddressSize::Bits16 {
        code.i32_const(0xffff)
            .i32_and()
            .local_get(register(pointer))
            .i32_const(!0xffff)
            .i32_and()
            .i32_or();
    }
    code.local_set(register(pointer));
}

fn read(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    width: StringWidth,
    imports: memory::Imports,
    exit_depth: u32,
) {
    // element() already checked both segment spans before the first page access.
    memory::effective_address(code, address, imports.raw_task_context.is_some());
    match width {
        StringWidth::Byte => {
            memory::load_narrow_at_address(code, SmallWidth::Byte, imports, exit_depth)
        }
        StringWidth::Word => {
            memory::load_narrow_at_address(code, SmallWidth::Word, imports, exit_depth)
        }
        StringWidth::Dword => {
            memory::load_result_at_address(code, imports, exit_depth);
            code.local_get(RESULT);
        }
    }
}

fn write(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    width: StringWidth,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::effective_address(code, address, imports.raw_task_context.is_some());
    match width {
        StringWidth::Byte => memory::store_byte_result_at_address(code, imports, exit_depth),
        StringWidth::Word => memory::store_word_result_at_address(code, imports, exit_depth),
        StringWidth::Dword => memory::store_result_at_address(code, imports, exit_depth),
    }
}

fn width_mask(width: StringWidth) -> i32 {
    match width {
        StringWidth::Byte => 0xff,
        StringWidth::Word => 0xffff,
        StringWidth::Dword => -1,
    }
}

fn accumulator(code: &mut InstructionSink<'_>, width: StringWidth) {
    code.local_get(register(Register32::Eax));
    if width != StringWidth::Dword {
        code.i32_const(width_mask(width)).i32_and();
    }
}

fn compare_flags(code: &mut InstructionSink<'_>, width: StringWidth) {
    let sign_bit = match width {
        StringWidth::Byte => 7,
        StringWidth::Word => 15,
        StringWidth::Dword => 31,
    };
    code.local_get(LHS).local_get(RHS).i32_sub();
    if width != StringWidth::Dword {
        code.i32_const(width_mask(width)).i32_and();
    }
    code.local_set(RESULT);
    code.local_get(FLAGS)
        .i32_const(!0x8d7)
        .i32_and()
        .i32_const(2)
        .i32_or()
        .local_get(LHS)
        .local_get(RHS)
        .i32_lt_u()
        .i32_or()
        .local_get(RESULT)
        .i32_const(0xff)
        .i32_and()
        .i32_popcnt()
        .i32_const(1)
        .i32_and()
        .i32_eqz()
        .i32_const(2)
        .i32_shl()
        .i32_or()
        .local_get(LHS)
        .local_get(RHS)
        .i32_xor()
        .local_get(RESULT)
        .i32_xor()
        .i32_const(0x10)
        .i32_and()
        .i32_or()
        .local_get(RESULT)
        .i32_eqz()
        .i32_const(6)
        .i32_shl()
        .i32_or()
        .local_get(RESULT)
        .i32_const(sign_bit - 7)
        .i32_shr_u()
        .i32_const(0x80)
        .i32_and()
        .i32_or()
        .local_get(LHS)
        .local_get(RHS)
        .i32_xor()
        .local_get(LHS)
        .local_get(RESULT)
        .i32_xor()
        .i32_and()
        .i32_const(sign_bit)
        .i32_shr_u()
        .i32_const(11)
        .i32_shl()
        .i32_or()
        .local_set(FLAGS);
}
