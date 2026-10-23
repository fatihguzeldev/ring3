use super::numeric;
use wasm_encoder::{
    CodeSection, ConstExpr, EntityType, ExportKind, ExportSection, FunctionSection, GlobalSection,
    GlobalType, ImportSection, NameMap, TypeSection, ValType,
};

pub(in crate::cpu::core) const ABI: i32 = 5;
pub(in crate::cpu::core) const NAMES: [&str; 12] = [
    "support.flags",
    "support.materialize_numeric",
    "support.resolve_status",
    "support.translate_page",
    "support.acquire_group",
    "support.store8",
    "support.store16",
    "support.store32",
    "support.commit_pool_cold",
    "support.commit_native",
    "support.commit_raw",
    "support.record_trace",
];

const COUNT: u32 = NAMES.len() as u32;

pub(in crate::cpu::core) fn types(types: &mut TypeSection) -> [u32; 12] {
    let first = types.len();
    types.ty().function([ValType::I32; 8], [ValType::I32]);
    types.ty().function([ValType::I32], []);
    types.ty().function(
        [ValType::I32; 2]
            .into_iter()
            .chain([ValType::F64; 3])
            .collect::<Vec<_>>(),
        [ValType::I32],
    );
    types.ty().function([ValType::I32; 3], [ValType::I32; 3]);
    types.ty().function([ValType::I32; 9], [ValType::I32; 3]);
    types.ty().function(
        [ValType::I32; 6]
            .into_iter()
            .chain([ValType::I64])
            .collect::<Vec<_>>(),
        [ValType::I32, ValType::I64],
    );
    types.ty().function(
        numeric::normalizer_kernel_parameters(),
        numeric::normalizer_kernel_results(),
    );
    types.ty().function(
        [
            ValType::I32,
            ValType::I32,
            ValType::F64,
            ValType::I32,
            ValType::I32,
        ],
        [ValType::I32],
    );
    types.ty().function(
        [ValType::I32, ValType::I32, ValType::I64, ValType::I32],
        [ValType::I32],
    );
    types.ty().function(super::trace::parameters(), []);
    [
        first,
        first + 1,
        first + 2,
        first + 3,
        first + 4,
        first + 5,
        first + 5,
        first + 5,
        first + 6,
        first + 7,
        first + 8,
        first + 9,
    ]
}

// All imports are direct functions of the same CPU's dispatcher instance. The
// immutable ABI global negotiates this complete set independently of link ABI2.
pub(super) fn imports(imports: &mut ImportSection, types: [u32; 12]) {
    for (name, kind) in NAMES.into_iter().zip(types) {
        imports.import("ring3", name, EntityType::Function(kind));
    }
    imports.import(
        "ring3",
        "support.abi",
        EntityType::Global(GlobalType {
            val_type: ValType::I32,
            mutable: false,
            shared: false,
        }),
    );
}

pub(in crate::cpu::core) fn define(
    functions: &mut FunctionSection,
    types: [u32; 12],
    globals: &mut GlobalSection,
    exports: &mut ExportSection,
    first: u32,
) {
    for (offset, (name, kind)) in NAMES.into_iter().zip(types).enumerate() {
        functions.function(kind);
        exports.export(name, ExportKind::Func, first + offset as u32);
    }
    let abi_global = globals.len();
    globals.global(
        GlobalType {
            val_type: ValType::I32,
            mutable: false,
            shared: false,
        },
        &ConstExpr::i32_const(ABI),
    );
    exports.export("supportAbi", ExportKind::Global, abi_global);
}

pub(in crate::cpu::core) fn bodies(code: &mut CodeSection, first: u32) {
    code.function(&super::super::flags::emit_function());
    code.function(&numeric::pool::emit_materializer(first + 2));
    code.function(&numeric::span::emit_status_resolver());
    code.function(&super::super::memory::cache::emit_translator());
    code.function(&super::super::memory::groups::emit_acquire());
    for width in [1, 2, 4] {
        code.function(&super::super::memory::emit_store(width));
    }
    code.function(&numeric::emit_normalizer_kernel());
    code.function(&numeric::record::emit_native_writer());
    code.function(&numeric::record::emit_raw_writer());
    code.function(&super::trace::emit_recorder());
}

pub(in crate::cpu::core) fn names(names: &mut NameMap, first: u32) {
    for (offset, name) in NAMES.into_iter().enumerate() {
        names.append(first + offset as u32, name);
    }
}

pub(super) fn owner_defined(offset: u32, numeric: bool) -> u32 {
    2 + u32::from(numeric) + COUNT + offset
}

// Keep the lowerer's logical helper IDs centralized while imports and owner
// definitions occupy separate contiguous Wasm function-index ranges.
pub(super) fn owner_internal(logical: u32, numeric: bool) -> u32 {
    let first = 2 + u32::from(numeric);
    match logical {
        2 => first,
        3..=5 => owner_defined(logical - 3, numeric),
        7..=9 => first + logical - 6,
        10 => owner_defined(3, numeric),
        11 => first + 4,
        12 => first + 9,
        13 => first + 10,
        14 => owner_defined(4, numeric),
        15 => first + 11,
        _ => unreachable!("unknown owner helper {logical}"),
    }
}

pub(super) fn owner_store(width: u32, numeric: bool) -> u32 {
    let offset = match width {
        1 => 5,
        2 => 6,
        4 => 7,
        _ => unreachable!("unsupported store width {width}"),
    };
    2 + u32::from(numeric) + offset
}

pub(super) fn owner_kernel(numeric: bool) -> u32 {
    2 + u32::from(numeric) + 8
}
