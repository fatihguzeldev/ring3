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

#[derive(Clone, Copy)]
enum TranslationSource {
    Local(u32),
    Constant(u32),
}

impl TranslationSource {
    fn emit(self, function: &mut Function) {
        match self {
            Self::Local(local) => {
                function.instructions().local_get(local);
            }
            Self::Constant(value) => {
                function.instructions().i32_const(value as i32);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum TranslationExit {
    Return,
    Inline([u32; 3]),
}

fn translation_body(
    function: &mut Function,
    address: TranslationSource,
    width: TranslationSource,
    required: TranslationSource,
    [page, slot, permission]: [u32; 3],
    exit: TranslationExit,
) {
    address.emit(function);
    function
        .instructions()
        .i32_const(12)
        .i32_shr_u()
        .local_set(page);
    present(function, &[76, 80, 84]);
    address.emit(function);
    function
        .instructions()
        .i32_const(4095)
        .i32_and()
        .i32_const(4096);
    width.emit(function);
    function
        .instructions()
        .i32_sub()
        .i32_le_u()
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(0)
        .i32_load(mem(76))
        .local_get(page)
        .i32_const(2)
        .i32_shl()
        .i32_add()
        .i32_load(mem(0))
        .local_tee(slot)
        .i32_eqz()
        .i32_eqz()
        .local_get(slot)
        .local_get(0)
        .i32_load(mem(88))
        .i32_le_u()
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(0)
        .i32_load(mem(80))
        .local_get(page)
        .i32_add()
        .i32_load8_u(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        })
        .local_tee(permission);
    required.emit(function);
    function.instructions().i32_const(8).i32_or().i32_and();
    required.emit(function);
    function
        .instructions()
        .i32_eq()
        .if_(BlockType::Empty)
        .local_get(slot)
        .i32_const(1)
        .i32_sub()
        .local_tee(slot)
        .i32_const(12)
        .i32_shl()
        .local_get(0)
        .i32_load(mem(84))
        .i32_add();
    address.emit(function);
    function
        .instructions()
        .i32_const(4095)
        .i32_and()
        .i32_add()
        .local_get(slot)
        .local_get(permission)
        .i32_const(3)
        .i32_and();
    match exit {
        TranslationExit::Return => {
            function.instructions().return_();
        }
        TranslationExit::Inline([host, slot, flags]) => {
            // all three tuple values are ready before scratch destinations change.
            // three translation checks are open; leave the inline wrapper.
            function
                .instructions()
                .local_set(flags)
                .local_set(slot)
                .local_set(host)
                .br(3);
        }
    }
    function
        .instructions()
        .end()
        .end()
        .end()
        .i32_const(0)
        .i32_const(0)
        .i32_const(0);
}

pub(super) fn translation() -> Function {
    let mut function = Function::new([(3, ValType::I32)]);
    translation_body(
        &mut function,
        TranslationSource::Local(1),
        TranslationSource::Local(2),
        TranslationSource::Local(3),
        [4, 5, 6],
        TranslationExit::Return,
    );
    function.instructions().end();
    function
}

pub(super) fn inline_translation(
    function: &mut Function,
    address: u32,
    width: u32,
    required: u32,
    scratch: [u32; 3],
    result: [u32; 3],
) {
    for local in scratch {
        function.instructions().i32_const(0).local_set(local);
    }
    function.instructions().block(BlockType::Empty);
    translation_body(
        function,
        TranslationSource::Local(address),
        TranslationSource::Constant(width),
        TranslationSource::Constant(required),
        scratch,
        TranslationExit::Inline(result),
    );
    for local in result.into_iter().rev() {
        function.instructions().local_set(local);
    }
    function.instructions().end();
    for local in result {
        function.instructions().local_get(local);
    }
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
