//! pure value primitives have no memory, architectural state or native calls.
use wasm_encoder::{BlockType, Function, ValType};

pub(super) fn load64() -> Function {
    let mut function = Function::new([
        (1, ValType::I64), // magnitude1
        (1, ValType::I32), // shift2
        (1, ValType::I64), // significand3
        (1, ValType::I32), // sign/exponent4
        (1, ValType::F64), // native5
        (2, ValType::I32), // valid6, certificate7
    ]);
    let mut c = function.instructions();
    c.local_get(0)
        .i64_const(0)
        .i64_lt_s()
        .if_(BlockType::Result(ValType::I64))
        .i64_const(0)
        .local_get(0)
        .i64_sub()
        .else_()
        .local_get(0)
        .end()
        .local_set(1)
        .local_get(0)
        .f64_convert_i64_s()
        .local_set(5)
        .local_get(1)
        .i64_eqz()
        .if_(BlockType::Empty)
        .i32_const(1)
        .local_set(6)
        .else_()
        .local_get(1)
        .i64_clz()
        .i32_wrap_i64()
        .local_set(2)
        .local_get(1)
        .local_get(2)
        .i64_extend_i32_u()
        .i64_shl()
        .local_set(3)
        .i32_const(16383 + 63)
        .local_get(2)
        .i32_sub()
        .local_get(0)
        .i64_const(0)
        .i64_lt_s()
        .i32_const(15)
        .i32_shl()
        .i32_or()
        .local_set(4)
        .local_get(2)
        .i32_const(11)
        .i32_ge_u()
        .if_(BlockType::Result(ValType::I32))
        .i32_const(1)
        .else_()
        .local_get(1)
        .i64_const(1)
        .i32_const(11)
        .local_get(2)
        .i32_sub()
        .i64_extend_i32_u()
        .i64_shl()
        .i64_const(1)
        .i64_sub()
        .i64_and()
        .i64_eqz()
        .end()
        .local_set(6)
        .end()
        .local_get(6)
        .local_set(7)
        .local_get(3)
        .local_get(4)
        .local_get(5)
        .local_get(6)
        .local_get(7)
        .end();
    function
}

pub(super) fn store(width: u32, truncate: bool) -> Function {
    assert!(matches!(width, 2 | 4 | 8));
    let bits = width * 8;
    let limit = match width {
        2 => 32768.0,
        4 => 2147483648.0,
        8 => 9223372036854775808.0,
        _ => unreachable!(),
    };
    let mut function = Function::new([
        (1, ValType::I32), // mode3
        (1, ValType::F64), // rounded4
        (1, ValType::I32), // status5
        (1, ValType::I64), // bits6
        (1, ValType::I32), // ok7
    ]);
    let mut c = function.instructions();
    c.local_get(2)
        .local_set(5)
        .local_get(0)
        .f64_abs()
        .f64_const(f64::INFINITY.into())
        .f64_lt()
        .if_(BlockType::Empty);
    if truncate {
        c.local_get(0).f64_trunc();
    } else {
        c.local_get(1)
            .i32_const(10)
            .i32_shr_u()
            .i32_const(3)
            .i32_and()
            .local_tee(3)
            .i32_eqz()
            .if_(BlockType::Result(ValType::F64))
            .local_get(0)
            .f64_nearest()
            .else_()
            .local_get(3)
            .i32_const(1)
            .i32_eq()
            .if_(BlockType::Result(ValType::F64))
            .local_get(0)
            .f64_floor()
            .else_()
            .local_get(3)
            .i32_const(2)
            .i32_eq()
            .if_(BlockType::Result(ValType::F64))
            .local_get(0)
            .f64_ceil()
            .else_()
            .local_get(0)
            .f64_trunc()
            .end()
            .end()
            .end();
    }
    c.local_set(4)
        .local_get(2)
        .i32_const(!0x200)
        .i32_and()
        .local_set(5)
        .local_get(4)
        .f64_const((-limit).into())
        .f64_ge()
        .local_get(4)
        .f64_const(limit.into())
        .f64_lt()
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(4)
        .i64_trunc_f64_s()
        .local_set(6)
        .local_get(4)
        .local_get(0)
        .f64_ne()
        .if_(BlockType::Empty)
        .local_get(5)
        .i32_const(0x20)
        .i32_or()
        .local_set(5)
        .end()
        .local_get(4)
        .f64_abs()
        .local_get(0)
        .f64_abs()
        .f64_gt()
        .if_(BlockType::Empty)
        .local_get(5)
        .i32_const(0x200)
        .i32_or()
        .local_set(5)
        .end()
        .else_()
        .i64_const((1_u64 << (bits - 1)) as i64)
        .local_set(6)
        .local_get(5)
        .i32_const(1)
        .i32_or()
        .local_set(5)
        .end()
        .i32_const(1)
        .local_set(7)
        .end()
        .local_get(6)
        .local_get(5)
        .local_get(7)
        .end();
    function
}
