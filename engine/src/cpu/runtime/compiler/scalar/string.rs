use super::*;

const VALUE: u32 = 32;
const STRIDE: u32 = 33;

pub(super) fn supports(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::String { .. } | Operation::RepeatString { .. }
    )
}

pub(super) fn emit(out: &mut Output, operation: Operation) -> bool {
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
        _ => return false,
    };
    let width = match width {
        StringWidth::Byte => 1,
        StringWidth::Word => 2,
        StringWidth::Dword => 4,
    };
    let source = EffectiveAddress {
        base: Some(Register32::Esi),
        segment: Some(source_segment.unwrap_or(SegmentRegister::Ds)),
        address_size,
        ..EffectiveAddress::default()
    };
    let destination = EffectiveAddress {
        base: Some(Register32::Edi),
        segment: Some(SegmentRegister::Es),
        address_size,
        ..EffectiveAddress::default()
    };
    out.c()
        .local_get(FLAGS)
        .i32_const(0x400)
        .i32_and()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(-(width as i32))
        .else_()
        .i32_const(width as i32)
        .end()
        .local_set(STRIDE)
        .block(BlockType::Empty);
    if repeat.is_some() {
        out.string_count(address_size);
        out.c().i32_eqz().br_if(0);
    }
    out.c().loop_(BlockType::Empty);
    if matches!(
        kind,
        StringKind::Move | StringKind::Load | StringKind::Compare
    ) {
        out.address(source);
        out.segment(width, false);
    }
    if kind != StringKind::Load {
        out.address(destination);
        out.segment(width, matches!(kind, StringKind::Move | StringKind::Store));
    }
    match kind {
        StringKind::Move => {
            out.address(source);
            out.read(width, VALUE);
            out.address(destination);
            out.write(width, VALUE);
        }
        StringKind::Store => {
            out.address(destination);
            out.write(width, reg(Register32::Eax));
        }
        StringKind::Load => {
            out.address(source);
            out.read(width, VALUE);
            match width {
                1 => out.store_byte(ByteRegister::Al, VALUE),
                2 => out.store_word(Register32::Eax, VALUE),
                _ => {
                    out.c().local_get(VALUE).local_set(reg(Register32::Eax));
                }
            }
        }
        StringKind::Compare | StringKind::Scan => {
            if kind == StringKind::Compare {
                out.address(source);
                out.read(width, A);
            } else {
                out.c().local_get(reg(Register32::Eax));
                if width != 4 {
                    out.c()
                        .i32_const(if width == 1 { 255 } else { 0xffff })
                        .i32_and();
                }
                out.c().local_set(A);
            }
            out.address(destination);
            out.read(width, B);
            out.binary(BinaryKind::Cmp);
            if width != 4 {
                out.c()
                    .local_get(R)
                    .i32_const(if width == 1 { 255 } else { 0xffff })
                    .i32_and()
                    .local_set(R);
            }
            out.flags(BinaryKind::Cmp, width * 8, false);
        }
    }
    if matches!(
        kind,
        StringKind::Move | StringKind::Load | StringKind::Compare
    ) {
        out.string_advance(Register32::Esi, address_size);
    }
    if kind != StringKind::Load {
        out.string_advance(Register32::Edi, address_size);
    }
    if let Some(repeat) = repeat {
        out.c()
            .local_get(reg(Register32::Ecx))
            .i32_const(1)
            .i32_sub();
        if address_size == AddressSize::Bits16 {
            out.c()
                .i32_const(0xffff)
                .i32_and()
                .local_get(reg(Register32::Ecx))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or();
        }
        out.c().local_set(reg(Register32::Ecx));
        out.string_count(address_size);
        out.c().i32_eqz().i32_eqz();
        if matches!(kind, StringKind::Compare | StringKind::Scan) {
            match repeat {
                RepeatCondition::Equal => {
                    out.bit(6);
                    out.c().i32_and();
                }
                RepeatCondition::NotEqual => {
                    out.bit(6);
                    out.c().i32_eqz().i32_and();
                }
                RepeatCondition::Always => {}
            }
        }
        out.c()
            .i32_eqz()
            .br_if(1)
            .local_get(REMAIN)
            .i32_const(1)
            .i32_sub()
            .local_set(REMAIN)
            .local_get(WRITE_STATUS)
            .i32_const(6)
            .i32_eq()
            .if_(BlockType::Empty);
        out.exit(6);
        out.c()
            .end()
            .local_get(REMAIN)
            .i32_eqz()
            .if_(BlockType::Empty);
        out.exit(2);
        out.c().end().br(0);
    } else {
        out.c().br(1);
    }
    out.c().end().end();
    true
}

impl Output {
    fn string_count(&mut self, size: AddressSize) {
        self.c().local_get(reg(Register32::Ecx));
        if size == AddressSize::Bits16 {
            self.c().i32_const(0xffff).i32_and();
        }
    }
    fn string_advance(&mut self, register: Register32, size: AddressSize) {
        self.c()
            .local_get(reg(register))
            .local_get(STRIDE)
            .i32_add();
        if size == AddressSize::Bits16 {
            self.c()
                .i32_const(0xffff)
                .i32_and()
                .local_get(reg(register))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or();
        }
        self.c().local_set(reg(register));
    }
}
