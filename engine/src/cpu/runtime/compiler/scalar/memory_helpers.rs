use super::*;

fn present(function: &mut Function, fields: &[u64]) {
    for (index, &field) in fields.iter().enumerate() {
        function
            .instructions()
            .local_get(0)
            .i32_load(mem(field))
            .i32_eqz()
            .i32_eqz();
        if index != 0 {
            function.instructions().i32_and();
        }
    }
}

pub(super) fn translation() -> Function {
    let mut function = Function::new([(3, ValType::I32)]);
    const PAGE: u32 = 4;
    const SLOT: u32 = 5;
    const PERMISSION: u32 = 6;
    function
        .instructions()
        .local_get(1)
        .i32_const(12)
        .i32_shr_u()
        .local_set(PAGE);
    present(&mut function, &[76, 80, 84]);
    function
        .instructions()
        .local_get(1)
        .i32_const(4095)
        .i32_and()
        .i32_const(4096)
        .local_get(2)
        .i32_sub()
        .i32_le_u()
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(0)
        .i32_load(mem(76))
        .local_get(PAGE)
        .i32_const(2)
        .i32_shl()
        .i32_add()
        .i32_load(mem(0))
        .local_tee(SLOT)
        .i32_eqz()
        .i32_eqz()
        .local_get(SLOT)
        .local_get(0)
        .i32_load(mem(88))
        .i32_le_u()
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(0)
        .i32_load(mem(80))
        .local_get(PAGE)
        .i32_add()
        .i32_load8_u(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        })
        .local_tee(PERMISSION)
        .local_get(3)
        .i32_const(8)
        .i32_or()
        .i32_and()
        .local_get(3)
        .i32_eq()
        .if_(BlockType::Empty)
        .local_get(SLOT)
        .i32_const(1)
        .i32_sub()
        .local_tee(SLOT)
        .i32_const(12)
        .i32_shl()
        .local_get(0)
        .i32_load(mem(84))
        .i32_add()
        .local_get(1)
        .i32_const(4095)
        .i32_and()
        .i32_add()
        .local_get(SLOT)
        .local_get(PERMISSION)
        .i32_const(3)
        .i32_and()
        .return_()
        .end()
        .end()
        .end()
        .i32_const(0)
        .i32_const(0)
        .i32_const(0)
        .end();
    function
}

pub(super) fn admission() -> Function {
    let mut function = Function::new([(3, ValType::I32)]);
    const LENGTH: u32 = 3;
    const BIT: u32 = 4;
    const READY: u32 = 5;
    present(&mut function, &[92, 100, 104, 156]);
    function
        .instructions()
        .if_(BlockType::Empty)
        .local_get(0)
        .i32_load(mem(108))
        .local_tee(LENGTH)
        .local_get(0)
        .i32_load(mem(112))
        .i32_le_u()
        .local_get(2)
        .local_get(0)
        .i32_load(mem(112))
        .i32_lt_u()
        .i32_and()
        .local_get(0)
        .i32_load(mem(156))
        .i64_load(MemArg {
            offset: 0,
            align: 3,
            memory_index: 0,
        })
        .i64_const(-1)
        .i64_ne()
        .i32_and()
        .local_get(0)
        .i32_load(mem(92))
        .local_get(2)
        .i32_const(2)
        .i32_shl()
        .i32_add()
        .i32_load(mem(0))
        .i32_eqz()
        .i32_and()
        .local_get(0)
        .i32_load(mem(96))
        .local_tee(BIT)
        .if_(BlockType::Result(ValType::I32))
        .local_get(BIT)
        .local_get(1)
        .i32_const(2)
        .i32_shl()
        .i32_add()
        .i32_load(mem(0))
        .i32_eqz()
        .else_()
        .i32_const(1)
        .end()
        .i32_and()
        .local_set(READY)
        .end()
        .local_get(READY)
        .if_(BlockType::Empty)
        .local_get(0)
        .i32_load(mem(100))
        .local_get(2)
        .i32_add()
        .i32_load8_u(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        })
        .local_tee(BIT)
        .i32_eqz()
        .if_(BlockType::Empty)
        .local_get(LENGTH)
        .local_get(0)
        .i32_load(mem(112))
        .i32_lt_u()
        .local_set(READY)
        .end()
        .end()
        .local_get(READY)
        .local_get(LENGTH)
        .local_get(BIT)
        .end();
    function
}
