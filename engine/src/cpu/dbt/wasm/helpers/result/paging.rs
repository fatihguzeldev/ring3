use super::*;

#[derive(Clone, Copy)]
pub(in crate::cpu::dbt::wasm) enum Geometry {
    Span { length: u32, mask: Option<u32> },
    OwnerChecked,
}

pub(in crate::cpu::dbt::wasm) struct Validation {
    pub(in crate::cpu::dbt::wasm) length: u32,
    pub(in crate::cpu::dbt::wasm) access_mask: u32,
    pub(in crate::cpu::dbt::wasm) allow_smc: bool,
    pub(in crate::cpu::dbt::wasm) geometry: Geometry,
}

pub(in crate::cpu::dbt::wasm) fn is_packet(code: &mut InstructionSink<'_>) {
    helper_field(code, 4);
    code.i32_const(0x1000f).i32_eq();
    code.local_get(HELPER_STATUS)
        .i32_const(23)
        .i32_eq()
        .i32_or();
}

pub(in crate::cpu::dbt::wasm) fn intent(code: &mut InstructionSink<'_>) {
    helper_field(code, 32);
    code.i32_const(0xff).i32_and();
}

pub(in crate::cpu::dbt::wasm) fn transport(code: &mut InstructionSink<'_>, allow_smc: bool) {
    code.local_get(HELPER_STATUS)
        .i32_eqz()
        .i32_eqz()
        .local_get(HELPER_STATUS)
        .i32_const(23)
        .i32_ne()
        .i32_and()
        .if_(BlockType::Empty);
    if allow_smc {
        code.local_get(HELPER_STATUS)
            .i32_const(11)
            .i32_eq()
            .if_(BlockType::Empty);
        infrastructure(code, 2);
        code.else_();
        infrastructure(code, 3);
        code.end();
    } else {
        infrastructure(code, 3);
    }
    code.end();
}

/// typed failures carry no success payload or SMC commit. only the native detector certifies PF.
pub(in crate::cpu::dbt::wasm) fn validate(code: &mut InstructionSink<'_>, validation: Validation) {
    let Validation {
        length,
        access_mask,
        allow_smc,
        geometry,
    } = validation;
    assert!(access_mask <= 3);
    code.local_get(HELPER_STATUS)
        .i32_eqz()
        .i32_eqz()
        .local_get(HELPER_STATUS)
        .i32_const(23)
        .i32_ne()
        .i32_and()
        .if_(BlockType::Empty);
    if allow_smc {
        code.local_get(HELPER_STATUS)
            .i32_const(11)
            .i32_eq()
            .if_(BlockType::Empty);
        infrastructure(code, 2);
        code.else_();
        infrastructure(code, 3);
        code.end();
    } else {
        infrastructure(code, 3);
    }
    code.else_();
    reset_checks(code);
    for (offset, expected) in [
        (0, u32::from_le_bytes(*b"R3MH")),
        (4, 0x1000f),
        (8, 40),
        (12, 0),
        (20, 0),
    ] {
        expect_field(code, offset, expected);
    }
    expect_field(code, 36, length);
    if length == 0 || access_mask == 0 {
        code.i32_const(1);
        add_check(code);
    }
    helper_field(code, 32);
    code.i32_const(0xff).i32_and().local_set(RHS);
    code.local_get(RHS)
        .i32_const(1)
        .i32_sub()
        .i32_const(1)
        .i32_gt_u();
    add_check(code);
    code.local_get(RHS)
        .i32_const(access_mask as i32)
        .i32_and()
        .local_get(RHS)
        .i32_ne();
    add_check(code);
    if let Geometry::Span { length, mask } = geometry {
        helper_field(code, 28);
        code.local_get(ADDRESS)
            .i32_sub()
            .local_tee(RESULT)
            .i32_const(length as i32)
            .i32_ge_u();
        add_check(code);
        if let Some(mask) = mask {
            code.local_get(mask)
                .local_get(RESULT)
                .i32_shr_u()
                .i32_const(1)
                .i32_and()
                .i32_eqz();
            add_check(code);
            if length < 32 {
                code.local_get(mask)
                    .i32_const(!((1_u32 << length) - 1) as i32)
                    .i32_and();
                add_check(code);
            }
        }
    }
    helper_field(code, 16);
    code.i32_const(9).i32_eq().if_(BlockType::Empty);
    code.local_get(HELPER_STATUS).i32_const(23).i32_ne();
    add_check(code);
    helper_field(code, 32);
    code.local_get(RHS).i32_ne();
    add_check(code);
    helper_field(code, 24);
    code.i32_const(7).i32_gt_u();
    add_check(code);
    helper_field(code, 24);
    code.i32_const(6)
        .i32_and()
        .local_get(RHS)
        .i32_const(2)
        .i32_eq()
        .i32_const(1)
        .i32_shl()
        .i32_const(4)
        .i32_or()
        .i32_ne();
    add_check(code);
    code.local_get(LHS).if_(BlockType::Empty);
    infrastructure(code, 2);
    code.else_().i32_const(23).local_set(REASON);
    for (offset, local) in [(24, DETAIL), (28, FAULT_ADDRESS), (32, FAULT_ACCESS)] {
        helper_field(code, offset);
        code.local_set(local);
    }
    code.i32_const(length as i32).local_set(FAULT_LENGTH).end();
    code.else_();
    infrastructure(code, 2);
    code.end().end();
}

#[cfg(test)]
mod tests;
