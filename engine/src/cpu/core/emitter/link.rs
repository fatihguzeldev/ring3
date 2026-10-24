use super::{CTX, Output, mem};
use crate::cpu::core::flags;
use wasm_encoder::{
    BlockType, ConstExpr, EntityType, ExportKind, ExportSection, Function, GlobalSection,
    GlobalType, ImportSection, InstructionSink, ValType,
};

pub(in crate::cpu::core) const MODE: u32 = 0;
pub(in crate::cpu::core) const VALID: u32 = 1;
pub(in crate::cpu::core) const CONTEXT: u32 = 2;
pub(in crate::cpu::core) const TRACE_ARENA: u32 = 22;
pub(in crate::cpu::core) const GLOBAL_COUNT: u32 = 23;
pub(in crate::cpu::core) const FIELDS: &[(u32, ValType)] = &[
    (2, ValType::I32),
    (3, ValType::I32),
    (4, ValType::I32),
    (5, ValType::I32),
    (6, ValType::I32),
    (7, ValType::I32),
    (8, ValType::I32),
    (9, ValType::I32),
    (10, ValType::I32),
    (11, ValType::I32),
    (12, ValType::I32),
    (13, ValType::I32),
    (20, ValType::I32),
    (21, ValType::I32),
    (22, ValType::I32),
    (23, ValType::I32),
    (24, ValType::I32),
    (25, ValType::I32),
    (99, ValType::I64),
];

pub(in crate::cpu::core) fn global(local: u32) -> u32 {
    3 + FIELDS
        .iter()
        .position(|&(field, _)| field == local)
        .unwrap() as u32
}

pub(in crate::cpu::core) fn name(index: u32) -> String {
    match index {
        MODE => "link.mode".into(),
        VALID => "link.valid".into(),
        CONTEXT => "link.context".into(),
        TRACE_ARENA => "trace.arena".into(),
        _ => format!("link.local{}", FIELDS[(index - 3) as usize].0),
    }
}

pub(in crate::cpu::core) fn imports(imports: &mut ImportSection) {
    for index in 0..GLOBAL_COUNT {
        let val_type = if index < 3 || index == TRACE_ARENA {
            ValType::I32
        } else {
            FIELDS[(index - 3) as usize].1
        };
        imports.import(
            "ring3",
            &name(index),
            EntityType::Global(GlobalType {
                val_type,
                mutable: true,
                shared: false,
            }),
        );
    }
}

pub(in crate::cpu::core) fn define_globals(
    globals: &mut GlobalSection,
    exports: &mut ExportSection,
    base: u32,
) {
    for index in 0..GLOBAL_COUNT {
        let val_type = if index < 3 || index == TRACE_ARENA {
            ValType::I32
        } else {
            FIELDS[(index - 3) as usize].1
        };
        let value = match val_type {
            ValType::I32 => ConstExpr::i32_const(0),
            ValType::I64 => ConstExpr::i64_const(0),
            ValType::F64 => ConstExpr::f64_const(0.0.into()),
            _ => unreachable!(),
        };
        globals.global(
            GlobalType {
                val_type,
                mutable: true,
                shared: false,
            },
            &value,
        );
        exports.export(&name(index), ExportKind::Global, base + index);
    }
    globals.global(
        GlobalType {
            val_type: ValType::I32,
            mutable: false,
            shared: false,
        },
        &ConstExpr::i32_const(4),
    );
    exports.export("linkAbi", ExportKind::Global, base + GLOBAL_COUNT);
}

pub(super) fn active(code: &mut InstructionSink<'_>) {
    code.global_get(MODE);
}

pub(super) fn has_state(code: &mut InstructionSink<'_>) {
    code.global_get(MODE)
        .global_get(VALID)
        .i32_and()
        .global_get(CONTEXT)
        .local_get(CTX)
        .i32_eq()
        .i32_and();
}

pub(super) fn restore(out: &mut Output<'_>) {
    for (local, _) in boundary_fields(!out.native.is_empty()) {
        out.c().global_get(global(local)).local_set(local);
    }
}

pub(super) fn boundary_fields(_numeric: bool) -> impl Iterator<Item = (u32, ValType)> {
    FIELDS.iter().copied()
}

fn publish_pool(code: &mut InstructionSink<'_>, fp_function: u32) {
    code.local_get(0)
        .i32_load(mem(super::numeric::BANK_POINTER, 2))
        .if_(BlockType::Empty)
        .local_get(0)
        .i32_load(mem(super::numeric::BANK_POINTER, 2))
        .call(fp_function)
        .end();
}

pub(super) fn import_canonical(out: &mut Output<'_>) {
    out.c()
        .global_get(MODE)
        .i32_eqz()
        .if_(BlockType::Empty)
        .local_get(CTX)
        .i32_load(mem(super::numeric::BANK_POINTER, 2))
        .local_tee(super::A)
        .if_(BlockType::Empty)
        .local_get(super::A)
        .i32_const(0)
        .i32_store(mem(
            crate::cpu::core::fp_pool::BANK_OFFSET + crate::cpu::core::fp_pool::ACTIVE,
            2,
        ))
        .end()
        .end();
}

pub(super) fn emit_boundary(
    numeric: bool,
    flags_function: u32,
    fp_function: u32,
    _resolver_function: u32,
) -> Function {
    let fields: Vec<_> = boundary_fields(numeric).collect();
    let parameter = |local| {
        fields
            .iter()
            .position(|&(field, _)| field == local)
            .unwrap() as u32
            + 1
    };
    let mut function = Function::new([]);
    let mut code = function.instructions();
    code.global_get(MODE).if_(BlockType::Empty);
    for &(local, _) in &fields {
        code.local_get(parameter(local)).global_set(global(local));
    }
    code.local_get(0)
        .global_set(CONTEXT)
        .i32_const(1)
        .global_set(VALID)
        .else_();
    publish_pool(&mut code, fp_function);
    for local in [11, 20, 25, 21, 22, 23, 24] {
        code.local_get(parameter(local));
    }
    code.i32_const(flags::ARITHMETIC as i32)
        .call(flags_function)
        .local_set(parameter(11));
    for (offset, local) in
        (0..8)
            .map(|index| (index * 4, index + 2))
            .chain([(32, 10), (36, 11), (40, 13), (44, 12)])
    {
        code.local_get(0)
            .local_get(parameter(local))
            .i32_store(mem(u64::from(offset), 2));
    }
    code.local_get(0)
        .i32_load(mem(crate::cpu::core::memory::TRANSACTION_VERSION, 2))
        .local_get(parameter(99))
        .i64_store(mem(0, 3))
        .end()
        .end();
    function
}

pub(super) fn emit_entry(body: u32, linked: bool, numeric: bool) -> Function {
    let mut function = Function::new([(1, ValType::I32)]);
    let mut code = function.instructions();
    code.i32_const(i32::from(linked)).global_set(MODE);
    if !linked {
        code.i32_const(0).global_set(VALID);
    }
    code.loop_(BlockType::Empty)
        .local_get(0)
        .local_get(1)
        .call(body)
        .local_tee(2)
        .i32_const(crate::cpu::core::StopReason::ExactPrefix as i32)
        .i32_eq()
        .if_(BlockType::Empty);
    // The generated body commits its actual completed prefix once. Linked
    // state is canonicalized only for this uncommon precise ingress.
    if linked {
        code.local_get(0)
            .call(super::support::owner_defined(7, numeric));
    }
    code.local_get(0)
        .call(2 + u32::from(numeric))
        .local_tee(2)
        .i32_const(crate::cpu::core::StopReason::NeedCode as i32)
        .i32_eq()
        .local_get(0)
        .i32_load(mem(32, 2))
        .local_get(0)
        .i32_load(mem(44, 2))
        .call(super::support::owner_defined(8, numeric))
        .i32_and()
        .if_(BlockType::Empty)
        .local_get(0)
        .i32_load(mem(44, 2))
        .local_set(1)
        .br(2)
        .end();
    if linked {
        code.local_get(2)
            .i32_const(crate::cpu::core::StopReason::NeedCode as i32)
            .i32_eq()
            .if_(BlockType::Empty);
        restore_canonical_globals(&mut code);
        code.end();
    }
    code.end()
        .local_get(2)
        .i32_const(0)
        .global_set(MODE)
        .return_()
        .end()
        .unreachable()
        .end();
    function
}

fn restore_canonical_globals(code: &mut InstructionSink<'_>) {
    for (offset, local) in
        (0..8)
            .map(|index| (index * 4, index + 2))
            .chain([(32, 10), (36, 11), (40, 13), (44, 12)])
    {
        code.local_get(0)
            .i32_load(mem(u64::from(offset), 2))
            .global_set(global(local));
    }
    for local in [20, 21, 22, 23, 24, 25] {
        code.i32_const(0).global_set(global(local));
    }
    code.local_get(0)
        .i32_load(mem(crate::cpu::core::memory::TRANSACTION_VERSION, 2))
        .i64_load(mem(0, 3))
        .global_set(global(99))
        .local_get(0)
        .global_set(CONTEXT)
        .i32_const(1)
        .global_set(VALID);
}

pub(super) fn emit_publisher(
    flags_function: u32,
    fp_function: u32,
    _resolver_function: u32,
) -> Function {
    let mut function = Function::new([]);
    let mut code = function.instructions();
    code.global_get(VALID)
        .i32_eqz()
        .if_(BlockType::Empty)
        .return_()
        .end()
        .global_get(CONTEXT)
        .local_get(0)
        .i32_ne()
        .if_(BlockType::Empty)
        .unreachable()
        .end();
    publish_pool(&mut code, fp_function);
    for local in [11, 20, 25, 21, 22, 23, 24] {
        code.global_get(global(local));
    }
    code.i32_const(flags::ARITHMETIC as i32)
        .call(flags_function)
        .global_set(global(11));
    for (offset, local) in
        (0..8)
            .map(|index| (index * 4, index + 2))
            .chain([(32, 10), (36, 11), (40, 13), (44, 12)])
    {
        code.local_get(0)
            .global_get(global(local))
            .i32_store(mem(u64::from(offset), 2));
    }
    code.local_get(0)
        .i32_load(mem(crate::cpu::core::memory::TRANSACTION_VERSION, 2))
        .global_get(global(99))
        .i64_store(mem(0, 3));
    code.i32_const(0).global_set(VALID).end();
    function
}
