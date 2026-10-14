use super::*;

pub(super) fn validate(code: &mut InstructionSink<'_>, width: u32, operation: CpuScalarOperation) {
    assert!(matches!(width, 1 | 2 | 4));
    let access = if operation == CpuScalarOperation::Read {
        1
    } else {
        2
    };
    code.local_get(HELPER_STATUS)
        .i32_eqz()
        .local_get(HELPER_STATUS)
        .i32_const(CPU_PAGE_FAULT_TRANSPORT as i32)
        .i32_eq()
        .i32_or()
        .i32_eqz()
        .if_(BlockType::Empty);
    infrastructure(code, 3);
    code.else_();
    reset_checks(code);
    for (offset, expected) in [
        (0, u32::from_le_bytes(*b"R3MH")),
        (4, 0x1000f),
        (8, 40),
        (12, 0),
        (20, 0),
        (36, width),
    ] {
        expect_field(code, offset, expected);
    }
    helper_field(code, 16);
    code.i32_const(9)
        .i32_eq()
        .local_get(HELPER_STATUS)
        .i32_const(CPU_PAGE_FAULT_TRANSPORT as i32)
        .i32_eq()
        .i32_xor();
    add_check(code);
    helper_field(code, 28);
    code.local_get(ADDRESS)
        .i32_sub()
        .i32_const(width as i32)
        .i32_ge_u();
    add_check(code);
    helper_field(code, 16);
    code.i32_const(9).i32_eq().if_(BlockType::Empty);
    expect_field(code, 32, access);
    helper_field(code, 24);
    code.i32_const(7).i32_gt_u();
    add_check(code);
    helper_field(code, 24);
    code.i32_const(6)
        .i32_and()
        .i32_const(if access == 1 { 4 } else { 6 })
        .i32_ne();
    add_check(code);
    code.local_get(LHS).if_(BlockType::Empty);
    infrastructure(code, 2);
    code.else_().i32_const(23).local_set(REASON);
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
    code.else_();
    infrastructure(code, 2);
    code.end().end();
}
