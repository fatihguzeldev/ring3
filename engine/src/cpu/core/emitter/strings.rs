use super::{
    A, B, FLAGS, KIND, LEFT, Output, REMAIN, RESULT, RETIRED, RIGHT, VALUE, WIDTH, WRITE_STATUS,
};
use crate::cpu::{
    core::StopReason,
    x86::{Register32, SegmentRegister, ir::*},
};
use wasm_encoder::BlockType;

pub(super) fn supports(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::String { .. } | Operation::RepeatString { .. }
    )
}

pub(super) fn emit(out: &mut Output<'_>, operation: Operation, next: u32) {
    let (kind, width, address_size, source_segment, repeat) = match operation {
        Operation::String {
            kind,
            width,
            address_size,
            source_segment,
        } => (kind, width, address_size, source_segment, None),
        Operation::RepeatString {
            kind,
            width,
            address_size,
            source_segment,
            repeat,
        } => (kind, width, address_size, source_segment, Some(repeat)),
        _ => unreachable!("string instruction"),
    };
    let width = match width {
        StringWidth::Byte => 1,
        StringWidth::Word => 2,
        StringWidth::Dword => 4,
    };
    let source = EffectiveAddress {
        base: Some(Register32::Esi),
        segment: source_segment,
        address_size,
        ..Default::default()
    };
    let destination = EffectiveAddress {
        base: Some(Register32::Edi),
        segment: Some(SegmentRegister::Es),
        address_size,
        ..Default::default()
    };
    out.materialize(super::super::flags::ARITHMETIC);
    out.c().i32_const(0).local_set(KIND);
    out.count(out.prefix);
    out.prefix = 0;
    let stores = matches!(kind, StringKind::Move | StringKind::Store);
    let Some(repeat) = repeat else {
        element(out, kind, width, address_size, source, destination);
        out.count(1);
        if stores {
            invalidated(out, next);
        }
        return;
    };
    out.c().block(BlockType::Empty).loop_(BlockType::Empty);
    count(out, address_size);
    out.c().i32_eqz().if_(BlockType::Empty);
    out.count(1);
    out.c().br(2).end();
    out.c().local_get(REMAIN).i32_eqz().if_(BlockType::Empty);
    out.exit(StopReason::Budget, Some(out.pc), 0);
    out.c().end();
    element(out, kind, width, address_size, source, destination);
    count(out, address_size);
    out.c().i32_const(1).i32_sub();
    if address_size == AddressSize::Bits16 {
        out.c()
            .i32_const(0xffff)
            .i32_and()
            .local_get(3)
            .i32_const(!0xffff)
            .i32_and()
            .i32_or();
    }
    out.c()
        .local_set(3)
        .local_get(REMAIN)
        .i32_const(1)
        .i32_sub()
        .local_set(REMAIN);
    count(out, address_size);
    out.c().i32_eqz().local_set(A);
    if repeat != RepeatCondition::Always {
        out.condition(if repeat == RepeatCondition::Equal {
            Condition::NotEqual
        } else {
            Condition::Equal
        });
        out.c().local_get(A).i32_or().local_set(A);
    }
    out.c().local_get(A).if_(BlockType::Empty);
    out.c()
        .local_get(RETIRED)
        .i32_const(1)
        .i32_add()
        .local_set(RETIRED);
    if stores {
        invalidated(out, next);
    }
    out.c().br(2).end();
    if stores {
        invalidated(out, out.pc);
    }
    out.c().br(0).end().end();
}

fn count(out: &mut Output<'_>, address_size: AddressSize) {
    out.c().local_get(3);
    if address_size == AddressSize::Bits16 {
        out.c().i32_const(0xffff).i32_and();
    }
}

fn invalidated(out: &mut Output<'_>, pc: u32) {
    out.c()
        .local_get(WRITE_STATUS)
        .i32_const(2)
        .i32_eq()
        .if_(BlockType::Empty);
    out.exit(StopReason::CodeInvalidated, Some(pc), 0);
    out.c().end();
}

fn element(
    out: &mut Output<'_>,
    kind: StringKind,
    width: u32,
    address_size: AddressSize,
    source: EffectiveAddress,
    destination: EffectiveAddress,
) {
    let source_used = matches!(
        kind,
        StringKind::Move | StringKind::Load | StringKind::Compare
    );
    let destination_used = matches!(
        kind,
        StringKind::Move | StringKind::Store | StringKind::Compare | StringKind::Scan
    );
    if source_used {
        out.address(source);
        out.check_span(width);
    }
    if destination_used {
        out.address(destination);
        out.check_span(width);
    }
    match kind {
        StringKind::Move => {
            out.address(source);
            out.read(width, RESULT);
            out.address(destination);
            out.write(width, RESULT);
        }
        StringKind::Store => {
            out.c().local_get(2).local_set(RESULT);
            out.address(destination);
            out.write(width, RESULT);
        }
        StringKind::Load => {
            out.address(source);
            out.read(width, RESULT);
            accumulator(out, width);
        }
        StringKind::Compare => {
            out.address(source);
            out.read(width, A);
            out.address(destination);
            out.read(width, B);
            compare(out, width);
        }
        StringKind::Scan => {
            out.address(destination);
            out.read(width, B);
            out.c()
                .local_get(2)
                .i32_const(mask(width) as i32)
                .i32_and()
                .local_set(A);
            compare(out, width);
        }
    }
    if source_used {
        advance(out, 8, width, address_size);
    }
    if destination_used {
        advance(out, 9, width, address_size);
    }
}

fn compare(out: &mut Output<'_>, width: u32) {
    out.c()
        .local_get(A)
        .local_get(B)
        .i32_sub()
        .i32_const(mask(width) as i32)
        .i32_and()
        .local_set(RESULT)
        .i32_const(super::super::flags::FlagKind::Sub as i32)
        .local_set(KIND)
        .i32_const((width * 8) as i32)
        .local_set(WIDTH);
    for (source, target) in [(A, LEFT), (B, RIGHT), (RESULT, VALUE)] {
        out.c().local_get(source).local_set(target);
    }
}

fn accumulator(out: &mut Output<'_>, width: u32) {
    if width == 4 {
        out.c().local_get(RESULT).local_set(2);
    } else {
        out.c()
            .local_get(2)
            .i32_const(!mask(width) as i32)
            .i32_and()
            .local_get(RESULT)
            .i32_or()
            .local_set(2);
    }
}

fn advance(out: &mut Output<'_>, register: u32, width: u32, address_size: AddressSize) {
    out.c()
        .local_get(register)
        .i32_const(-(width as i32))
        .i32_const(width as i32)
        .local_get(FLAGS)
        .i32_const(0x400)
        .i32_and()
        .select()
        .i32_add();
    if address_size == AddressSize::Bits16 {
        out.c()
            .i32_const(0xffff)
            .i32_and()
            .local_get(register)
            .i32_const(!0xffff)
            .i32_and()
            .i32_or();
    }
    out.c().local_set(register);
}

fn mask(width: u32) -> u32 {
    u32::MAX >> (32 - width * 8)
}
