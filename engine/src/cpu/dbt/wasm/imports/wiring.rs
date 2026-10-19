use crate::cpu::dbt::wasm::{
    EmbeddedBinding,
    imports::{HelperBinding, Imports},
    requirements::ModuleRequirements,
};
use wasm_encoder::{EntityType, ImportSection, MemoryType, TypeSection, ValType};
pub(in crate::cpu::dbt::wasm) struct ImportSections {
    pub(in crate::cpu::dbt::wasm) types: TypeSection,
    pub(in crate::cpu::dbt::wasm) imports: ImportSection,
    pub(in crate::cpu::dbt::wasm) context: Imports,
    pub(in crate::cpu::dbt::wasm) run_index: u32,
    pub(in crate::cpu::dbt::wasm) segment_check_type: u32,
    pub(in crate::cpu::dbt::wasm) ordinary_validator_type: u32,
    pub(in crate::cpu::dbt::wasm) scalar_read32_validator_type: u32,
}

pub(in crate::cpu::dbt::wasm) fn build(
    plan: &ModuleRequirements,
    binding: Option<EmbeddedBinding>,
) -> ImportSections {
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32; 4], [ValType::I32]);
    if let Some(binding) = binding {
        let parameters = match binding {
            EmbeddedBinding::Replacement { .. } => 6,
            EmbeddedBinding::Resident { .. } => 7,
        };
        types.ty().function(
            std::iter::repeat_n(ValType::I32, parameters),
            [ValType::I64],
        );
    }
    if plan.has_reads {
        types.ty().function([ValType::I32], [ValType::I32]);
    }
    if plan.has_stores {
        let parameters = if matches!(binding, Some(EmbeddedBinding::Resident { .. })) {
            6
        } else {
            2
        };
        types.ty().function(
            std::iter::repeat_n(ValType::I32, parameters),
            [ValType::I32],
        );
    }
    if plan.imports.compare_exchange32
        || plan.imports.compare_exchange8
        || plan.imports.compare_exchange16
    {
        let parameters = if matches!(binding, Some(EmbeddedBinding::Resident { .. })) {
            7
        } else {
            3
        };
        types.ty().function(
            std::iter::repeat_n(ValType::I32, parameters),
            [ValType::I32],
        );
    }
    let cpu_type_base = 1
        + u32::from(binding.is_some())
        + u32::from(plan.has_reads)
        + u32::from(plan.has_stores)
        + u32::from(
            plan.imports.compare_exchange32
                || plan.imports.compare_exchange8
                || plan.imports.compare_exchange16,
        );
    if plan.has_cpu {
        let resident_extra = if matches!(binding, Some(EmbeddedBinding::Resident { .. })) {
            4
        } else {
            0
        };
        for parameters in [
            3,
            3 + resident_extra,
            6 + resident_extra,
            6 + resident_extra,
            1 + resident_extra,
            3 + resident_extra,
            2 + resident_extra,
            3 + resident_extra,
            2 + resident_extra,
            1 + resident_extra,
        ] {
            types.ty().function(
                std::iter::repeat_n(ValType::I32, parameters),
                [ValType::I32],
            );
        }
    }
    if plan.imports.far_call {
        let parameters = if matches!(binding, Some(EmbeddedBinding::Resident { .. })) {
            9
        } else {
            5
        };
        types.ty().function(
            std::iter::repeat_n(ValType::I32, parameters),
            [ValType::I32],
        );
    }
    let xstate_type_offset = 10 + u32::from(plan.imports.far_call);
    if plan.upper_xstate {
        let parameters = if matches!(binding, Some(EmbeddedBinding::Resident { .. })) {
            8
        } else {
            4
        };
        types.ty().function(
            std::iter::repeat_n(ValType::I32, parameters),
            [ValType::I32],
        );
    }
    let segment_check_type = cpu_type_base
        + if plan.has_cpu { 10 } else { 0 }
        + u32::from(plan.imports.far_call)
        + u32::from(plan.upper_xstate);
    if plan.has_segment_check {
        types.ty().function([ValType::I32; 6], [ValType::I32]);
    }
    let finish_detection_type = segment_check_type + u32::from(plan.has_segment_check);
    if plan.detects_exceptions {
        let mut parameters = [ValType::I32; 12];
        parameters[0] = ValType::I64;
        types.ty().function(parameters, [ValType::I32]);
    }
    let fetch_fault_type = finish_detection_type + u32::from(plan.detects_exceptions);

    if plan.needs_fetch_paging {
        let parameters = if matches!(binding, Some(EmbeddedBinding::Resident { .. })) {
            6
        } else {
            2
        };
        types.ty().function(
            std::iter::repeat_n(ValType::I32, parameters),
            [ValType::I32],
        );
    }
    let ordinary_validator_type = fetch_fault_type + u32::from(plan.needs_fetch_paging);
    let has_ordinary_validator = plan.has_zero_span_validator || plan.has_x87_read32_validator;
    if has_ordinary_validator {
        types.ty().function([ValType::I32; 12], [ValType::I32; 8]);
    }
    let scalar_read32_validator_type = ordinary_validator_type + u32::from(has_ordinary_validator);
    if plan.has_read32_validator {
        types.ty().function([ValType::I32; 10], [ValType::I32; 7]);
    }
    let mut imports = ImportSection::new();
    imports.import(
        "env",
        "memory",
        EntityType::Memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        }),
    );
    if let Some(binding) = binding {
        let name = match binding {
            EmbeddedBinding::Replacement { .. } => "guard_detect",
            EmbeddedBinding::Resident { .. } => "guard_detect_resident",
        };
        imports.import("ring3", name, EntityType::Function(1));
    }
    let mut helper_imports = Imports::default();
    let mut function_index = u32::from(binding.is_some());
    let mut type_index = 1 + u32::from(binding.is_some());
    let read_type_index = type_index;
    if plan.imports.read {
        imports.import("ring3", "cpu_read32", EntityType::Function(type_index));
        helper_imports.read = Some(function_index);
        function_index += 1;
        type_index += 1;
    } else if plan.has_reads {
        type_index += 1;
    }
    if plan.imports.store {
        let (name, store) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_store_resident32",
                HelperBinding::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => ("cpu_store32", HelperBinding::Replacement(function_index)),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.store = Some(store);
        function_index += 1;
    }
    if plan.imports.read8 {
        imports.import("ring3", "cpu_read8", EntityType::Function(read_type_index));
        helper_imports.read8 = Some(function_index);
        function_index += 1;
    }
    if plan.imports.read16 {
        imports.import("ring3", "cpu_read16", EntityType::Function(read_type_index));
        helper_imports.read16 = Some(function_index);
        function_index += 1;
    }
    for (needed, name, slot) in [
        (
            plan.modify[0],
            "cpu_read_modify8",
            &mut helper_imports.read_modify8,
        ),
        (
            plan.modify[1],
            "cpu_read_modify16",
            &mut helper_imports.read_modify16,
        ),
        (
            plan.modify[2],
            "cpu_read_modify32",
            &mut helper_imports.read_modify,
        ),
    ] {
        if needed {
            imports.import("ring3", name, EntityType::Function(read_type_index));
            *slot = Some(function_index);
            function_index += 1;
        }
    }
    if plan.imports.store8 {
        let (name, store) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_store_resident8",
                HelperBinding::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => ("cpu_store8", HelperBinding::Replacement(function_index)),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.store8 = Some(store);
        function_index += 1;
    }
    if plan.imports.store16 {
        let (name, store) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_store_resident16",
                HelperBinding::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => ("cpu_store16", HelperBinding::Replacement(function_index)),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.store16 = Some(store);
        function_index += 1;
    }
    if plan.imports.exchange {
        let (name, exchange) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_exchange_resident32",
                HelperBinding::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => ("cpu_exchange32", HelperBinding::Replacement(function_index)),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.exchange = Some(exchange);
        function_index += 1;
    }
    if plan.imports.exchange8 {
        let (name, exchange) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_exchange_resident8",
                HelperBinding::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => ("cpu_exchange8", HelperBinding::Replacement(function_index)),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.exchange8 = Some(exchange);
        function_index += 1;
    }
    if plan.imports.exchange16 {
        let (name, exchange) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_exchange_resident16",
                HelperBinding::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => ("cpu_exchange16", HelperBinding::Replacement(function_index)),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.exchange16 = Some(exchange);
        function_index += 1;
    }
    if plan.imports.compare_exchange32 {
        let (name, exchange) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_compare_exchange_resident32",
                HelperBinding::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_compare_exchange32",
                HelperBinding::Replacement(function_index),
            ),
        };
        imports.import(
            "ring3",
            name,
            EntityType::Function(type_index + u32::from(plan.has_stores)),
        );
        helper_imports.compare_exchange32 = Some(exchange);
        function_index += 1;
    }
    if plan.imports.exchange_add32 {
        let (name, exchange) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_exchange_add_resident32",
                HelperBinding::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_exchange_add32",
                HelperBinding::Replacement(function_index),
            ),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.exchange_add32 = Some(exchange);
        function_index += 1;
    }
    if plan.imports.compare_exchange8 {
        let (name, exchange) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_compare_exchange_resident8",
                HelperBinding::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_compare_exchange8",
                HelperBinding::Replacement(function_index),
            ),
        };
        imports.import(
            "ring3",
            name,
            EntityType::Function(type_index + u32::from(plan.has_stores)),
        );
        helper_imports.compare_exchange8 = Some(exchange);
        function_index += 1;
    }
    if plan.imports.exchange_add8 {
        let (name, exchange) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_exchange_add_resident8",
                HelperBinding::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_exchange_add8",
                HelperBinding::Replacement(function_index),
            ),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.exchange_add8 = Some(exchange);
        function_index += 1;
    }
    if plan.imports.compare_exchange16 {
        let (name, exchange) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_compare_exchange_resident16",
                HelperBinding::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_compare_exchange16",
                HelperBinding::Replacement(function_index),
            ),
        };
        imports.import(
            "ring3",
            name,
            EntityType::Function(type_index + u32::from(plan.has_stores)),
        );
        helper_imports.compare_exchange16 = Some(exchange);
        function_index += 1;
    }
    if plan.imports.exchange_add16 {
        let (name, exchange) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_exchange_add_resident16",
                HelperBinding::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_exchange_add16",
                HelperBinding::Replacement(function_index),
            ),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.exchange_add16 = Some(exchange);
        function_index += 1;
    }
    if plan.imports.bit_test_set32 {
        let (name, bit_set) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_bit_test_set_resident32",
                HelperBinding::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_bit_test_set32",
                HelperBinding::Replacement(function_index),
            ),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.bit_test_set32 = Some(bit_set);
        function_index += 1;
    }
    if plan.imports.bit_test_reset32 {
        let (name, bit_reset) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_bit_test_reset_resident32",
                HelperBinding::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_bit_test_reset32",
                HelperBinding::Replacement(function_index),
            ),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.bit_test_reset32 = Some(bit_reset);
        function_index += 1;
    }
    if plan.imports.atomic_or_u32 {
        let (name, atomic_or) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_atomic_or_resident_u32",
                HelperBinding::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_atomic_or_u32",
                HelperBinding::Replacement(function_index),
            ),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.atomic_or_u32 = Some(atomic_or);
        function_index += 1;
    }
    if plan.imports.atomic_and_u32 {
        let (name, atomic_and) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_atomic_and_resident_u32",
                HelperBinding::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_atomic_and_u32",
                HelperBinding::Replacement(function_index),
            ),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.atomic_and_u32 = Some(atomic_and);
        function_index += 1;
    }
    if plan.imports.atomic_xor_u32 {
        let (name, atomic_xor) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_atomic_xor_resident_u32",
                HelperBinding::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_atomic_xor_u32",
                HelperBinding::Replacement(function_index),
            ),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.atomic_xor_u32 = Some(atomic_xor);
        function_index += 1;
    }
    if plan.imports.cpu_read {
        imports.import("ring3", "cpu_read", EntityType::Function(cpu_type_base));
        helper_imports.cpu_read = Some(function_index);
        function_index += 1;
    }
    for (needed, name, resident_name, type_offset, slot) in [
        (
            plan.imports.stack_execute,
            "stack_execute",
            "stack_execute_resident",
            9,
            &mut helper_imports.stack_execute,
        ),
        (
            plan.imports.cpu_integer,
            "cpu_integer",
            "cpu_integer_resident",
            8,
            &mut helper_imports.cpu_integer,
        ),
        (
            plan.imports.avx_execute,
            "avx_execute",
            "avx_execute_resident",
            3,
            &mut helper_imports.avx_execute,
        ),
        (
            plan.imports.simd_execute,
            "simd_execute",
            "simd_execute_resident",
            3,
            &mut helper_imports.simd_execute,
        ),
        (
            plan.imports.cpu_write,
            "cpu_write",
            "cpu_write_resident",
            1,
            &mut helper_imports.cpu_write,
        ),
        (
            plan.imports.x87_op,
            "x87_op",
            "x87_resident_op",
            2,
            &mut helper_imports.x87_op,
        ),
        (
            plan.imports.cpu_fxsave,
            "cpu_fxsave",
            "cpu_fxsave_resident",
            4,
            &mut helper_imports.cpu_fxsave,
        ),
        (
            plan.imports.cpu_xsave,
            if plan.upper_xstate {
                "cpu_xsave_avx"
            } else {
                "cpu_xsave"
            },
            if plan.upper_xstate {
                "cpu_xsave_avx_resident"
            } else {
                "cpu_xsave_resident"
            },
            if plan.upper_xstate {
                xstate_type_offset
            } else {
                1
            },
            &mut helper_imports.cpu_xsave,
        ),
        (
            plan.imports.cpu_xrstor,
            if plan.upper_xstate {
                "cpu_xrstor_avx"
            } else {
                "cpu_xrstor"
            },
            if plan.upper_xstate {
                "cpu_xrstor_avx_resident"
            } else {
                "cpu_xrstor_resident"
            },
            if plan.upper_xstate {
                xstate_type_offset
            } else {
                1
            },
            &mut helper_imports.cpu_xrstor,
        ),
        (
            plan.imports.cpu_mask_write,
            "cpu_mask_write",
            "cpu_mask_write_resident",
            5,
            &mut helper_imports.cpu_mask_write,
        ),
        (
            plan.imports.segment_load,
            "segment_load",
            "segment_load_resident",
            6,
            &mut helper_imports.segment_load,
        ),
        (
            plan.imports.segment_load_far,
            "segment_load_far",
            "segment_load_far_resident",
            7,
            &mut helper_imports.segment_load_far,
        ),
        (
            plan.imports.far_call,
            "far_call",
            "far_call_resident",
            10,
            &mut helper_imports.far_call,
        ),
        (
            plan.imports.far_return,
            "far_return",
            "far_return_resident",
            7,
            &mut helper_imports.far_return,
        ),
        (
            plan.imports.far_jump,
            "far_jump",
            "far_jump_resident",
            7,
            &mut helper_imports.far_jump,
        ),
    ] {
        if needed {
            let (name, import) = match binding {
                Some(EmbeddedBinding::Resident { key, id }) => (
                    resident_name,
                    HelperBinding::Resident {
                        index: function_index,
                        key,
                        id,
                    },
                ),
                _ => (name, HelperBinding::Replacement(function_index)),
            };
            imports.import(
                "ring3",
                name,
                EntityType::Function(cpu_type_base + type_offset),
            );
            *slot = Some(import);
            function_index += 1;
        }
    }
    if plan.detects_exceptions {
        imports.import(
            "ring3",
            "finish_exception_detection",
            EntityType::Function(finish_detection_type),
        );
        helper_imports.finish_detection = Some(function_index);
        function_index += 1;
    }

    if plan.needs_fetch_paging {
        let (name, import) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_fetch_fault_resident",
                HelperBinding::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_fetch_fault",
                HelperBinding::Replacement(function_index),
            ),
        };
        imports.import("ring3", name, EntityType::Function(fetch_fault_type));
        helper_imports.fetch_fault = Some(import);
        function_index += 1;
    }

    ImportSections {
        types,
        imports,
        context: helper_imports,
        run_index: function_index,
        segment_check_type,
        ordinary_validator_type,
        scalar_read32_validator_type,
    }
}
