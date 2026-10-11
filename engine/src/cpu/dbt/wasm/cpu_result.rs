use wasm_encoder::{BlockType, InstructionSink, ValType};

use super::{
    locals::*,
    memory::{
        add_check, exit_if_failed, expect_field, helper_field, infrastructure, protocol_if_bad,
        reset_checks,
    },
};

pub(super) fn value(code: &mut InstructionSink<'_>) {
    helper_field(code, 20);
}

pub(super) fn validate(
    code: &mut InstructionSink<'_>,
    length: u32,
    access_mask: u32,
    allow_smc: bool,
    exit_depth: u32,
) {
    validate_span(code, length, access_mask, allow_smc, None, exit_depth);
}

pub(super) fn validate_masked(
    code: &mut InstructionSink<'_>,
    length: u32,
    mask_local: u32,
    exit_depth: u32,
) {
    assert!((1..=16).contains(&length));
    validate_span(code, length, 2, true, Some(mask_local), exit_depth);
}

fn validate_span(
    code: &mut InstructionSink<'_>,
    length: u32,
    access_mask: u32,
    allow_smc: bool,
    mask: Option<u32>,
    exit_depth: u32,
) {
    assert!(length <= 512);
    assert!(access_mask <= 3);
    code.local_get(HELPER_STATUS);
    if allow_smc {
        code.i32_const(11)
            .i32_ne()
            .local_get(HELPER_STATUS)
            .i32_eqz()
            .i32_eqz()
            .i32_and();
    }
    code.if_(BlockType::Empty);
    infrastructure(code, 3);
    code.else_();
    reset_checks(code);
    for (offset, expected) in [
        (0, u32::from_le_bytes(*b"R3MH")),
        (4, 0x10007),
        (8, 40),
        (12, 0),
        (36, length),
    ] {
        expect_field(code, offset, expected);
    }
    code.local_get(LHS).if_(BlockType::Empty);
    infrastructure(code, 2);
    code.else_();
    helper_field(code, 16);
    code.local_set(RHS);
    code.local_get(RHS).i32_eqz().if_(BlockType::Empty);
    success(code, length, mask);
    code.else_();
    code.local_get(HELPER_STATUS)
        .i32_const(11)
        .i32_eq()
        .if_(BlockType::Empty);
    infrastructure(code, 2);
    code.else_();
    code.local_get(RHS)
        .i32_const(1)
        .i32_eq()
        .if_(BlockType::Empty);
    fault(code, length, access_mask, mask);
    code.else_()
        .local_get(RHS)
        .i32_const(2)
        .i32_eq()
        .if_(BlockType::Empty);
    rejected(code);
    code.else_()
        .local_get(RHS)
        .i32_const(3)
        .i32_ge_u()
        .local_get(RHS)
        .i32_const(7)
        .i32_le_u()
        .i32_and()
        .if_(BlockType::Empty);
    exception(code);
    code.else_();
    infrastructure(code, 2);
    code.end().end().end().end().end().end().end();
    exit_if_failed(code, exit_depth);
}

fn success(code: &mut InstructionSink<'_>, length: u32, mask: Option<u32>) {
    reset_checks(code);
    for offset in [24, 28, 32] {
        expect_field(code, offset, 0);
    }
    if let Some(mask_local) = mask {
        masked_overflow(code, length, mask_local);
        add_check(code);
        code.local_get(mask_local)
            .i32_const(!((1u32 << length) - 1) as i32)
            .i32_and();
        add_check(code);
        code.local_get(HELPER_STATUS)
            .i32_const(11)
            .i32_eq()
            .local_get(mask_local)
            .i32_eqz()
            .i32_and();
        add_check(code);
    } else if length != 0 {
        code.local_get(ADDRESS)
            .i32_const(-(length as i32))
            .i32_gt_u();
        add_check(code);
    }
    protocol_if_bad(code);
}

fn fault(code: &mut InstructionSink<'_>, length: u32, access_mask: u32, mask: Option<u32>) {
    if length == 0 || access_mask == 0 {
        infrastructure(code, 2);
        return;
    }
    reset_checks(code);
    expect_field(code, 20, 0);
    helper_field(code, 32);
    code.i32_const(1).i32_sub().i32_const(1).i32_gt_u();
    add_check(code);
    helper_field(code, 32);
    code.i32_const(access_mask as i32).i32_and();
    helper_field(code, 32);
    code.i32_ne();
    add_check(code);
    helper_field(code, 24);
    code.i32_const(1).i32_sub().i32_const(2).i32_gt_u();
    add_check(code);
    helper_field(code, 24);
    code.i32_const(3)
        .i32_eq()
        .if_(BlockType::Result(ValType::I32));
    if let Some(mask_local) = mask {
        masked_overflow(code, length, mask_local);
        code.i32_eqz();
    } else {
        code.local_get(ADDRESS)
            .i32_const(-(length as i32))
            .i32_le_u();
    }
    helper_field(code, 28);
    code.local_get(ADDRESS).i32_ne().i32_or();
    code.else_();
    if let Some(mask_local) = mask {
        masked_overflow(code, length, mask_local);
        helper_field(code, 28);
        code.local_get(ADDRESS)
            .i32_sub()
            .local_tee(RESULT)
            .i32_const(length as i32 - 1)
            .i32_gt_u()
            .i32_or();
        code.local_get(mask_local)
            .local_get(RESULT)
            .i32_shr_u()
            .i32_const(1)
            .i32_and()
            .i32_eqz()
            .i32_or();
    } else {
        code.local_get(ADDRESS)
            .i32_const(-(length as i32))
            .i32_gt_u();
        helper_field(code, 28);
        code.local_get(ADDRESS)
            .i32_sub()
            .i32_const(length as i32 - 1)
            .i32_gt_u()
            .i32_or();
    }
    code.end();
    add_check(code);
    if let Some(mask_local) = mask {
        code.local_get(mask_local)
            .i32_const(!((1u32 << length) - 1) as i32)
            .i32_and();
        add_check(code);
    }
    code.local_get(LHS).if_(BlockType::Empty);
    infrastructure(code, 2);
    code.else_();
    code.i32_const(5).local_set(REASON);
    for (offset, local) in [
        (24, DETAIL),
        (28, FAULT_ADDRESS),
        (32, FAULT_ACCESS),
        (36, FAULT_LENGTH),
    ] {
        helper_field(code, offset);
        code.local_set(local);
    }
    code.end();
}

fn rejected(code: &mut InstructionSink<'_>) {
    reset_checks(code);
    for offset in [20, 28, 32] {
        expect_field(code, offset, 0);
    }
    helper_field(code, 24);
    code.i32_const(1).i32_sub().i32_const(1).i32_gt_u();
    add_check(code);
    code.local_get(LHS).if_(BlockType::Empty);
    infrastructure(code, 2);
    code.else_();
    helper_field(code, 24);
    code.i32_const(1).i32_eq().if_(BlockType::Empty);
    infrastructure(code, 1);
    code.else_();
    infrastructure(code, 3);
    code.end().end();
}

fn exception(code: &mut InstructionSink<'_>) {
    reset_checks(code);
    for offset in [20, 28, 32] {
        expect_field(code, offset, 0);
    }
    code.local_get(RHS)
        .i32_const(4)
        .i32_eq()
        .local_get(RHS)
        .i32_const(5)
        .i32_eq()
        .i32_or()
        .if_(BlockType::Empty);
    helper_field(code, 24);
    code.i32_const(0xffff).i32_gt_u();
    add_check(code);
    code.end();
    code.local_get(LHS).if_(BlockType::Empty);
    infrastructure(code, 2);
    code.else_();
    code.local_get(RHS).i32_const(8).i32_add().local_set(REASON);
    helper_field(code, 24);
    code.local_set(DETAIL);
    code.end();
}

fn masked_overflow(code: &mut InstructionSink<'_>, length: u32, mask_local: u32) {
    code.i32_const(0);
    for offset in 1..length {
        code.local_get(mask_local)
            .i32_const((1u32 << offset) as i32)
            .i32_and()
            .i32_eqz()
            .i32_eqz();
        code.local_get(ADDRESS)
            .i32_const(-(offset as i32) - 1)
            .i32_gt_u()
            .i32_and()
            .i32_or();
    }
}
