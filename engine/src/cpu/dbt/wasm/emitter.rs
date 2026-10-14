use wasm_encoder::{
    BlockType, CodeSection, ConstExpr, EntityType, ExportKind, ExportSection, Function,
    FunctionSection, GlobalSection, GlobalType, ImportSection, InstructionSink, MemoryType, Module,
    TypeSection, ValType,
};

use super::{EmbeddedBinding, abi, code_fetch, integer, locals::*, memory, segments, x87};
use crate::cpu::dbt::region::CompiledBlock;
use crate::cpu::x86::ir::Operation;

#[cfg(test)]
pub(in crate::cpu::dbt) fn emit(
    blocks: &[CompiledBlock],
    binding: Option<EmbeddedBinding>,
    decode_key: Option<crate::cpu::x86::decode::CodeDecodeKey>,
) -> Vec<u8> {
    emit_with_authority(blocks, binding, decode_key, false, false)
}

pub(in crate::cpu::dbt) fn emit_with_authority(
    blocks: &[CompiledBlock],
    binding: Option<EmbeddedBinding>,
    decode_key: Option<crate::cpu::x86::decode::CodeDecodeKey>,
    task_return_configured: bool,
    exception_detection_configured: bool,
) -> Vec<u8> {
    let detects_exceptions = exception_detection_configured && binding.is_some();
    let device_not_available =
        blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .any(|instruction| {
                matches!(
                    instruction.operation(),
                    Operation::Control(crate::cpu::x86::ControlOperation::DeviceNotAvailable)
                )
            });
    let needs_task_return = task_return_configured
        && blocks
            .iter()
            .flat_map(|b| &b.instructions)
            .any(|i| matches!(i.operation(), Operation::InterruptReturn { .. }));
    let mut module = Module::new();
    let memory::ImportNeeds {
        stack_execute: needs_stack_execute,
        cpu_integer: needs_cpu_integer,
        cpu_read: needs_cpu_read,
        cpu_write: needs_cpu_write,
        x87_op: needs_x87_op,
        simd_execute: needs_simd_execute,
        avx_execute: needs_avx_execute,
        cpu_fxsave: needs_cpu_fxsave,
        cpu_xsave: needs_cpu_xsave,
        cpu_xrstor: needs_cpu_xrstor,
        cpu_mask_write: needs_cpu_mask_write,
        segment_load: needs_segment_load,
        segment_load_far: needs_segment_load_far,
        far_jump: needs_far_jump,
        far_call: needs_far_call,
        far_return: needs_far_return,
        software_interrupt: needs_software_interrupt,
        read: needs_read,
        store: needs_store,
        read8: needs_read8,
        read16: needs_read16,
        store8: needs_store8,
        store16: needs_store16,
        exchange: needs_exchange,
        exchange8: needs_exchange8,
        exchange16: needs_exchange16,
        compare_exchange32: needs_compare_exchange32,
        exchange_add32: needs_exchange_add32,
        compare_exchange8: needs_compare_exchange8,
        exchange_add8: needs_exchange_add8,
        compare_exchange16: needs_compare_exchange16,
        exchange_add16: needs_exchange_add16,
        bit_test_set32: needs_bit_test_set32,
        bit_test_reset32: needs_bit_test_reset32,
        atomic_or_u32: needs_atomic_or_u32,
        atomic_and_u32: needs_atomic_and_u32,
        atomic_xor_u32: needs_atomic_xor_u32,
    } = memory::Imports::needed(blocks);
    let has_stores = needs_store
        || needs_store8
        || needs_store16
        || needs_exchange
        || needs_exchange8
        || needs_exchange16
        || needs_exchange_add32
        || needs_exchange_add8
        || needs_exchange_add16
        || needs_bit_test_set32
        || needs_bit_test_reset32
        || needs_atomic_or_u32
        || needs_atomic_and_u32
        || needs_atomic_xor_u32;
    let has_reads = needs_read || needs_read8 || needs_read16;
    let has_avx = blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .any(|instruction| matches!(instruction.operation(), Operation::Avx(_)));
    let has_simd = blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .any(|instruction| {
            matches!(
                instruction.operation(),
                Operation::Simd(_)
                    | Operation::Avx(_)
                    | Operation::PackedStringCompare(_)
                    | Operation::XState(
                        crate::cpu::x86::xstate::XStateOperation::Save { .. }
                            | crate::cpu::x86::xstate::XStateOperation::Restore { .. }
                    )
            )
        });
    let has_xstate = blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .any(|instruction| matches!(instruction.operation(), Operation::XState(_)));
    let has_control_fault =
        blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .any(|instruction| {
                matches!(
                    instruction.operation(),
                    Operation::Control(
                        crate::cpu::x86::ControlOperation::GeneralProtectionZero
                            | crate::cpu::x86::ControlOperation::DeniedPortString { .. }
                    ) | Operation::XState(_)
                )
            });
    let has_cpu = has_simd
        || has_control_fault
        || needs_stack_execute
        || needs_cpu_integer
        || needs_cpu_read
        || needs_cpu_write
        || needs_x87_op
        || needs_simd_execute
        || needs_avx_execute
        || needs_cpu_fxsave
        || needs_cpu_xsave
        || needs_cpu_xrstor
        || needs_cpu_mask_write
        || needs_segment_load
        || needs_segment_load_far
        || needs_far_jump
        || needs_far_call
        || needs_far_return
        || needs_software_interrupt;
    let has_memory = has_cpu
        || has_reads
        || has_stores
        || needs_compare_exchange32
        || needs_compare_exchange8
        || needs_compare_exchange16;
    let has_gates = blocks.iter().any(|block| block.gate().is_some());
    let has_arithmetic = blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .any(|instruction| {
            matches!(
                instruction.operation(),
                Operation::DivideAccumulator { .. }
                    | Operation::AsciiAdjustAfterMultiplication { .. }
                    | Operation::WordDivideAccumulator { .. }
                    | Operation::ReadWordDivideAccumulator { .. }
                    | Operation::ByteDivideAccumulator { .. }
                    | Operation::ReadByteDivideAccumulator { .. }
                    | Operation::ReadDivideAccumulator { .. }
            )
        });
    let has_x87 = blocks.iter().any(|block| {
        block.instructions.iter().any(|instruction| {
            matches!(
                instruction.operation(),
                Operation::XState(
                    crate::cpu::x86::xstate::XStateOperation::Save { .. }
                        | crate::cpu::x86::xstate::XStateOperation::Restore { .. }
                ) | Operation::X87 { .. }
                    | Operation::InitializeX87
                    | Operation::ClearX87Exceptions
                    | Operation::X87StatusToAx
                    | Operation::X87StatusToMemory { .. }
                    | Operation::X87ControlToMemory { .. }
            )
        })
    });
    debug_assert!(!(has_memory || has_gates) || binding.is_some());
    let has_repeat = blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .any(|instruction| {
            matches!(
                instruction.operation(),
                Operation::RepeatString { .. }
                    | Operation::Control(crate::cpu::x86::ControlOperation::DeniedPortString {
                        repeat_count: Some(_),
                    })
            )
        });
    let has_segments = binding.is_some()
        || blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .any(|instruction| {
                instruction.uses_segments()
                    || matches!(
                        instruction.operation(),
                        Operation::StackFrame { .. }
                            | Operation::PopAll
                            | Operation::PopAllWord
                            | Operation::EnterZero
                            | Operation::EnterZeroWord
                            | Operation::ReadSegment { .. }
                            | Operation::PushSegment { .. }
                            | Operation::LoadSegment { .. }
                            | Operation::PopSegment { .. }
                            | Operation::LoadFarPointer { .. }
                            | Operation::FarJump { .. }
                            | Operation::FarCall { .. }
                            | Operation::FarReturn { .. }
                            | Operation::InterruptReturn { .. }
                            | Operation::SoftwareInterrupt { .. }
                    )
            });
    let has_segment_check =
        blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .any(|instruction| {
                instruction.segment_accesses() != 0
                    || matches!(instruction.operation(), Operation::InterruptReturn { .. })
            });
    let mut types = TypeSection::new();
    types.ty().function([ValType::I32; 4], [ValType::I32]);
    if let Some(binding) = binding {
        let parameters = match binding {
            EmbeddedBinding::Replacement { .. } => 6,
            EmbeddedBinding::Resident { .. } => 7,
        };
        types.ty().function(
            std::iter::repeat_n(ValType::I32, parameters),
            [if detects_exceptions {
                ValType::I64
            } else {
                ValType::I32
            }],
        );
    }
    if has_reads {
        types.ty().function([ValType::I32], [ValType::I32]);
    }
    if has_stores {
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
    if needs_compare_exchange32 || needs_compare_exchange8 || needs_compare_exchange16 {
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
        + u32::from(has_reads)
        + u32::from(has_stores)
        + u32::from(
            needs_compare_exchange32 || needs_compare_exchange8 || needs_compare_exchange16,
        );
    if has_cpu {
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
    if needs_far_call {
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
    let interrupt_type_offset = 10 + u32::from(needs_far_call);
    if needs_software_interrupt {
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
    let task_type_offset = 10 + u32::from(needs_far_call) + u32::from(needs_software_interrupt);
    if needs_task_return {
        let parameters = if matches!(binding, Some(EmbeddedBinding::Resident { .. })) {
            4
        } else {
            0
        };
        types.ty().function(
            std::iter::repeat_n(ValType::I32, parameters),
            [ValType::I32],
        );
    }
    let upper_xstate =
        crate::cpu::x86::capabilities::AVX_ENABLED && (needs_cpu_xsave || needs_cpu_xrstor);
    let xstate_type_offset = 10
        + u32::from(needs_far_call)
        + u32::from(needs_software_interrupt)
        + u32::from(needs_task_return);
    if upper_xstate {
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
        + if has_cpu { 10 } else { 0 }
        + u32::from(needs_far_call)
        + u32::from(upper_xstate)
        + u32::from(needs_software_interrupt)
        + u32::from(needs_task_return);
    if has_segment_check {
        types.ty().function([ValType::I32; 6], [ValType::I32]);
    }
    let finish_detection_type = segment_check_type + u32::from(has_segment_check);
    if detects_exceptions {
        let mut parameters = [ValType::I32; 12];
        parameters[0] = ValType::I64;
        types.ty().function(parameters, [ValType::I32]);
    }
    module.section(&types);
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
            EmbeddedBinding::Replacement { .. } if detects_exceptions => "guard_detect",
            EmbeddedBinding::Resident { .. } if detects_exceptions => "guard_detect_resident",
            EmbeddedBinding::Replacement { .. } => "guard",
            EmbeddedBinding::Resident { .. } => "guard_resident",
        };
        imports.import("ring3", name, EntityType::Function(1));
    }
    let mut helper_imports = memory::Imports::default();
    let mut function_index = u32::from(binding.is_some());
    let mut type_index = 1 + u32::from(binding.is_some());
    let read_type_index = type_index;
    if needs_read {
        imports.import("ring3", "cpu_read32", EntityType::Function(type_index));
        helper_imports.read = Some(function_index);
        function_index += 1;
        type_index += 1;
    } else if has_reads {
        type_index += 1;
    }
    if needs_store {
        let (name, store) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_store_resident32",
                memory::StoreImport::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_store32",
                memory::StoreImport::Replacement(function_index),
            ),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.store = Some(store);
        function_index += 1;
    }
    if needs_read8 {
        imports.import("ring3", "cpu_read8", EntityType::Function(read_type_index));
        helper_imports.read8 = Some(function_index);
        function_index += 1;
    }
    if needs_read16 {
        imports.import("ring3", "cpu_read16", EntityType::Function(read_type_index));
        helper_imports.read16 = Some(function_index);
        function_index += 1;
    }
    if needs_store8 {
        let (name, store) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_store_resident8",
                memory::StoreImport::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_store8",
                memory::StoreImport::Replacement(function_index),
            ),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.store8 = Some(store);
        function_index += 1;
    }
    if needs_store16 {
        let (name, store) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_store_resident16",
                memory::StoreImport::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_store16",
                memory::StoreImport::Replacement(function_index),
            ),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.store16 = Some(store);
        function_index += 1;
    }
    if needs_exchange {
        let (name, exchange) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_exchange_resident32",
                memory::StoreImport::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_exchange32",
                memory::StoreImport::Replacement(function_index),
            ),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.exchange = Some(exchange);
        function_index += 1;
    }
    if needs_exchange8 {
        let (name, exchange) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_exchange_resident8",
                memory::StoreImport::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_exchange8",
                memory::StoreImport::Replacement(function_index),
            ),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.exchange8 = Some(exchange);
        function_index += 1;
    }
    if needs_exchange16 {
        let (name, exchange) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_exchange_resident16",
                memory::StoreImport::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_exchange16",
                memory::StoreImport::Replacement(function_index),
            ),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.exchange16 = Some(exchange);
        function_index += 1;
    }
    if needs_compare_exchange32 {
        let (name, exchange) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_compare_exchange_resident32",
                memory::StoreImport::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_compare_exchange32",
                memory::StoreImport::Replacement(function_index),
            ),
        };
        imports.import(
            "ring3",
            name,
            EntityType::Function(type_index + u32::from(has_stores)),
        );
        helper_imports.compare_exchange32 = Some(exchange);
        function_index += 1;
    }
    if needs_exchange_add32 {
        let (name, exchange) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_exchange_add_resident32",
                memory::StoreImport::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_exchange_add32",
                memory::StoreImport::Replacement(function_index),
            ),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.exchange_add32 = Some(exchange);
        function_index += 1;
    }
    if needs_compare_exchange8 {
        let (name, exchange) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_compare_exchange_resident8",
                memory::StoreImport::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_compare_exchange8",
                memory::StoreImport::Replacement(function_index),
            ),
        };
        imports.import(
            "ring3",
            name,
            EntityType::Function(type_index + u32::from(has_stores)),
        );
        helper_imports.compare_exchange8 = Some(exchange);
        function_index += 1;
    }
    if needs_exchange_add8 {
        let (name, exchange) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_exchange_add_resident8",
                memory::StoreImport::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_exchange_add8",
                memory::StoreImport::Replacement(function_index),
            ),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.exchange_add8 = Some(exchange);
        function_index += 1;
    }
    if needs_compare_exchange16 {
        let (name, exchange) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_compare_exchange_resident16",
                memory::StoreImport::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_compare_exchange16",
                memory::StoreImport::Replacement(function_index),
            ),
        };
        imports.import(
            "ring3",
            name,
            EntityType::Function(type_index + u32::from(has_stores)),
        );
        helper_imports.compare_exchange16 = Some(exchange);
        function_index += 1;
    }
    if needs_exchange_add16 {
        let (name, exchange) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_exchange_add_resident16",
                memory::StoreImport::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_exchange_add16",
                memory::StoreImport::Replacement(function_index),
            ),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.exchange_add16 = Some(exchange);
        function_index += 1;
    }
    if needs_bit_test_set32 {
        let (name, bit_set) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_bit_test_set_resident32",
                memory::StoreImport::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_bit_test_set32",
                memory::StoreImport::Replacement(function_index),
            ),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.bit_test_set32 = Some(bit_set);
        function_index += 1;
    }
    if needs_bit_test_reset32 {
        let (name, bit_reset) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_bit_test_reset_resident32",
                memory::StoreImport::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_bit_test_reset32",
                memory::StoreImport::Replacement(function_index),
            ),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.bit_test_reset32 = Some(bit_reset);
        function_index += 1;
    }
    if needs_atomic_or_u32 {
        let (name, atomic_or) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_atomic_or_resident_u32",
                memory::StoreImport::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_atomic_or_u32",
                memory::StoreImport::Replacement(function_index),
            ),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.atomic_or_u32 = Some(atomic_or);
        function_index += 1;
    }
    if needs_atomic_and_u32 {
        let (name, atomic_and) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_atomic_and_resident_u32",
                memory::StoreImport::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_atomic_and_u32",
                memory::StoreImport::Replacement(function_index),
            ),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.atomic_and_u32 = Some(atomic_and);
        function_index += 1;
    }
    if needs_atomic_xor_u32 {
        let (name, atomic_xor) = match binding {
            Some(EmbeddedBinding::Resident { key, id }) => (
                "cpu_atomic_xor_resident_u32",
                memory::StoreImport::Resident {
                    index: function_index,
                    key,
                    id,
                },
            ),
            _ => (
                "cpu_atomic_xor_u32",
                memory::StoreImport::Replacement(function_index),
            ),
        };
        imports.import("ring3", name, EntityType::Function(type_index));
        helper_imports.atomic_xor_u32 = Some(atomic_xor);
        function_index += 1;
    }
    if needs_cpu_read {
        imports.import("ring3", "cpu_read", EntityType::Function(cpu_type_base));
        helper_imports.cpu_read = Some(function_index);
        function_index += 1;
    }
    for (needed, name, resident_name, type_offset, slot) in [
        (
            needs_stack_execute,
            "stack_execute",
            "stack_execute_resident",
            9,
            &mut helper_imports.stack_execute,
        ),
        (
            needs_cpu_integer,
            "cpu_integer",
            "cpu_integer_resident",
            8,
            &mut helper_imports.cpu_integer,
        ),
        (
            needs_avx_execute,
            "avx_execute",
            "avx_execute_resident",
            3,
            &mut helper_imports.avx_execute,
        ),
        (
            needs_simd_execute,
            "simd_execute",
            "simd_execute_resident",
            3,
            &mut helper_imports.simd_execute,
        ),
        (
            needs_cpu_write,
            "cpu_write",
            "cpu_write_resident",
            1,
            &mut helper_imports.cpu_write,
        ),
        (
            needs_x87_op,
            "x87_op",
            "x87_resident_op",
            2,
            &mut helper_imports.x87_op,
        ),
        (
            needs_cpu_fxsave,
            "cpu_fxsave",
            "cpu_fxsave_resident",
            4,
            &mut helper_imports.cpu_fxsave,
        ),
        (
            needs_cpu_xsave,
            if upper_xstate {
                "cpu_xsave_avx"
            } else {
                "cpu_xsave"
            },
            if upper_xstate {
                "cpu_xsave_avx_resident"
            } else {
                "cpu_xsave_resident"
            },
            if upper_xstate { xstate_type_offset } else { 1 },
            &mut helper_imports.cpu_xsave,
        ),
        (
            needs_cpu_xrstor,
            if upper_xstate {
                "cpu_xrstor_avx"
            } else {
                "cpu_xrstor"
            },
            if upper_xstate {
                "cpu_xrstor_avx_resident"
            } else {
                "cpu_xrstor_resident"
            },
            if upper_xstate { xstate_type_offset } else { 1 },
            &mut helper_imports.cpu_xrstor,
        ),
        (
            needs_cpu_mask_write,
            "cpu_mask_write",
            "cpu_mask_write_resident",
            5,
            &mut helper_imports.cpu_mask_write,
        ),
        (
            needs_segment_load,
            "segment_load",
            "segment_load_resident",
            6,
            &mut helper_imports.segment_load,
        ),
        (
            needs_segment_load_far,
            "segment_load_far",
            "segment_load_far_resident",
            7,
            &mut helper_imports.segment_load_far,
        ),
        (
            needs_far_call,
            "far_call",
            "far_call_resident",
            10,
            &mut helper_imports.far_call,
        ),
        (
            needs_software_interrupt,
            "interrupt_gate",
            "interrupt_gate_resident",
            interrupt_type_offset,
            &mut helper_imports.software_interrupt,
        ),
        (
            needs_task_return,
            "task_return",
            "task_return_resident",
            task_type_offset,
            &mut helper_imports.task_return,
        ),
        (
            needs_far_return,
            "far_return",
            "far_return_resident",
            7,
            &mut helper_imports.far_return,
        ),
        (
            needs_far_jump,
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
                    memory::StoreImport::Resident {
                        index: function_index,
                        key,
                        id,
                    },
                ),
                _ => (name, memory::StoreImport::Replacement(function_index)),
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
    if detects_exceptions {
        imports.import(
            "ring3",
            "finish_exception_detection",
            EntityType::Function(finish_detection_type),
        );
        helper_imports.finish_detection = Some(function_index);
        function_index += 1;
    }
    module.section(&imports);
    let mut functions = FunctionSection::new();
    functions.function(0);
    if has_segment_check {
        helper_imports.segment_check = Some(function_index + 1);
        functions.function(segment_check_type);
    }
    module.section(&functions);
    let key = decode_key.unwrap_or(crate::cpu::x86::decode::CodeDecodeKey::FLAT);
    let mut globals = GlobalSection::new();
    for value in [
        i32::from(binding.is_some()),
        key.cs_base as i32,
        i32::from(key.default32),
    ] {
        globals.global(
            GlobalType {
                val_type: ValType::I32,
                mutable: false,
                shared: false,
            },
            &ConstExpr::i32_const(value),
        );
    }
    module.section(&globals);
    let mut exports = ExportSection::new();
    exports.export("run", ExportKind::Func, function_index);
    module.section(&exports);
    let mut declarations = vec![(16, ValType::I32), (1, ValType::I64), (10, ValType::I32)];
    if blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .any(|instruction| {
            matches!(instruction.operation(), Operation::Simd(operation)
            if operation.kind == crate::cpu::x86::simd::SimdKind::Crypto(
                crate::cpu::x86::simd::CryptoKind::CarrylessMultiply))
        })
    {
        declarations.push((5, ValType::I64));
    }
    if binding.is_some() {
        let first = 4 + declarations.iter().map(|(count, _)| count).sum::<u32>();
        helper_imports.code_fetch_limit = Some(first);
        declarations.push((1, ValType::I32));
    }
    if blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .any(|instruction| matches!(instruction.operation(), Operation::InterruptReturn { .. }))
    {
        let first = 4 + declarations.iter().map(|(count, _)| count).sum::<u32>();
        helper_imports.interrupt_return_locals =
            Some(super::far::InterruptReturnLocals::new(first));
        declarations.push((3, ValType::I32));
    }
    if needs_task_return {
        let first = 4 + declarations.iter().map(|(count, _)| count).sum::<u32>();
        helper_imports.task_return_flags = Some(first);
        declarations.push((1, ValType::I32));
    }
    if needs_software_interrupt {
        let first = 4 + declarations.iter().map(|(count, _)| count).sum::<u32>();
        helper_imports.software_interrupt_flags = Some(first);
        declarations.push((1, ValType::I32));
    }
    if has_avx {
        let first = 4 + declarations.iter().map(|(count, _)| count).sum::<u32>();
        helper_imports.avx_locals = Some(super::avx::AvxLocals::new(first));
        declarations.push((super::avx::INTEGER_LOCALS, ValType::I32));
        declarations.push((super::avx::VECTOR_LOCALS, ValType::V128));
    }
    if detects_exceptions {
        let first = 4 + declarations.iter().map(|(count, _)| count).sum::<u32>();
        helper_imports.detection = Some(super::detection::DetectionLocals::new(first, has_repeat));
        declarations.push((1, ValType::I64));
        declarations.push((6, ValType::I32));
    }
    let mut function = Function::new(declarations);
    let mut code = function.instructions();
    if let Some(binding) = binding {
        match binding {
            EmbeddedBinding::Replacement { key, generation } => {
                code.i32_const(key as u32 as i32)
                    .i32_const((key >> 32) as u32 as i32)
                    .i32_const(generation as i32);
            }
            EmbeddedBinding::Resident { key, id } => {
                code.i32_const(key as u32 as i32)
                    .i32_const((key >> 32) as u32 as i32)
                    .i32_const(id as u32 as i32)
                    .i32_const((id >> 32) as u32 as i32);
            }
        }
        code.local_get(STATE_PTR)
            .local_get(EXIT_PTR)
            .local_get(CANCEL_PTR)
            .call(0);
        if let Some(detection) = helper_imports.detection {
            code.local_tee(detection.epoch)
                .i64_const(0)
                .i64_le_s()
                .if_(BlockType::Empty)
                .local_get(detection.epoch)
                .i64_const(-22)
                .i64_ge_s()
                .local_get(detection.epoch)
                .i64_const(0)
                .i64_lt_s()
                .i32_and()
                .if_(BlockType::Result(ValType::I32))
                .local_get(detection.epoch)
                .i64_const(-1)
                .i64_mul()
                .i32_wrap_i64()
                .else_()
                .i32_const(9)
                .end()
                .return_()
                .end();
        } else {
            code.local_tee(LHS)
                .if_(BlockType::Empty)
                .local_get(LHS)
                .return_()
                .end();
        }
    }
    abi::preflight(&mut code);
    if has_segments {
        super::segments::preflight(&mut code);
    }
    if has_repeat {
        abi::work_preflight(&mut code);
        code.local_get(BUDGET).local_set(ORIGINAL_BUDGET);
    }
    if has_x87 {
        x87::preflight(&mut code);
    }
    if has_simd {
        super::simd::preflight(&mut code);
    }
    if has_avx || upper_xstate {
        super::avx::preflight(&mut code);
    }
    abi::load_state(&mut code);
    if let Some(limit) = helper_imports.code_fetch_limit {
        code_fetch::refresh(&mut code, limit);
    }
    if let Some(locals) = helper_imports.avx_locals {
        super::avx::initialize(&mut code, locals);
    }
    code.block(BlockType::Empty).loop_(BlockType::Empty);
    abi::safepoint(&mut code, 1);
    if binding.is_some() {
        code_fetch::check_context(&mut code, 1);
    }
    for block in blocks {
        emit_block(&mut code, block, helper_imports);
    }
    if let Some(limit) = helper_imports.code_fetch_limit {
        code_fetch::check_pc(&mut code, limit, 1);
    }
    code.i32_const(3).local_set(REASON).br(1).end().end();
    if has_repeat {
        code.local_get(STATE_PTR)
            .local_get(ORIGINAL_BUDGET)
            .local_get(BUDGET)
            .i32_sub()
            .i32_store(wasm_encoder::MemArg {
                offset: crate::abi::arena::WORK_CONSUMED_OFFSET as u64,
                align: 2,
                memory_index: 0,
            });
    }
    abi::flush(
        &mut code,
        has_memory,
        has_gates,
        has_arithmetic,
        has_cpu
            || has_simd
            || has_reads
            || has_stores
            || needs_compare_exchange32
            || needs_compare_exchange8
            || needs_compare_exchange16,
        has_xstate,
        needs_task_return,
        device_not_available,
    );
    super::detection::finish(&mut code, helper_imports);
    code.i32_const(0).end();
    let mut bodies = CodeSection::new();
    bodies.function(&function);
    if has_segment_check {
        bodies.function(&segments::build_function());
    }
    module.section(&bodies);
    module.finish()
}

fn emit_block(code: &mut InstructionSink<'_>, block: &CompiledBlock, imports: memory::Imports) {
    if let Some(gate) = block.gate() {
        code.local_get(EIP)
            .i32_const(gate.entry.0 as i32)
            .i32_eq()
            .if_(BlockType::Empty);
        if let Some(limit) = imports.code_fetch_limit {
            code_fetch::check_context(code, 2);
            code_fetch::refresh(code, limit);
            code_fetch::check_span(code, gate.entry.0, 2, limit, 2);
        }
        code.i32_const(gate.id as i32)
            .local_set(DETAIL)
            .i32_const(8)
            .local_set(REASON)
            .br(2)
            .end();
        return;
    }
    if !block.instructions.is_empty() {
        code.i32_const(-1).local_set(RESUME).block(BlockType::Empty);
        for (index, instruction) in block.instructions.iter().enumerate() {
            code.local_get(EIP)
                .i32_const(instruction.pc().0 as i32)
                .i32_eq()
                .if_(BlockType::Empty)
                .i32_const(index as i32)
                .local_set(RESUME)
                .br(1)
                .end();
        }
        code.end()
            .local_get(RESUME)
            .i32_const(-1)
            .i32_ne()
            .if_(BlockType::Empty);
        let helper_free = code_fetch::helper_free_block(block);
        if helper_free && let Some(limit) = imports.code_fetch_limit {
            // the loop's safepoint and key check already precede this selected block.
            code_fetch::refresh(code, limit);
        }
        let count = block.instructions.len() as u32;
        for _ in 0..count {
            code.block(BlockType::Empty);
        }
        code.local_get(RESUME).br_table(0..count, count);
        for (index, instruction) in block.instructions.iter().enumerate() {
            code.end();
            let remaining = count - index as u32 - 1;
            abi::safepoint(code, remaining + 2);
            if let Some(limit) = imports.code_fetch_limit {
                if !helper_free {
                    code_fetch::check_context(code, remaining + 2);
                    code_fetch::refresh(code, limit);
                }
                code_fetch::check_instruction(code, instruction, limit, remaining + 2);
            }
            if matches!(instruction.operation(), Operation::InterruptReturn { .. }) {
                if let Some(original) = imports.task_return_flags {
                    code.local_get(FLAGS).local_set(original);
                } else {
                    abi::interrupt_return_precheck(code, remaining + 2);
                }
            }
            if matches!(
                instruction.operation(),
                Operation::Control(crate::cpu::x86::ControlOperation::DeviceNotAvailable)
            ) {
                super::control_instructions::instruction(code, instruction, imports, remaining + 2);
                continue;
            }
            x87::precheck_instruction(code, instruction, imports, remaining + 2);
            if matches!(instruction.operation(), Operation::SoftwareInterrupt { .. }) {
                code.local_get(FLAGS).local_set(
                    imports
                        .software_interrupt_flags
                        .expect("interrupt flag snapshot"),
                );
            }
            super::detection::snapshot(code, instruction, imports);
            abi::begin_instruction(code);
            if matches!(instruction.operation(), Operation::InterruptReturn { .. })
                && imports.task_return.is_some()
            {
                super::far::task::return_if_nested(code, imports, remaining + 2);
            }
            integer::instruction(code, instruction, imports, remaining + 2);
            if matches!(
                instruction.operation(),
                Operation::FarJump { .. }
                    | Operation::FarCall { .. }
                    | Operation::FarReturn { .. }
                    | Operation::SoftwareInterrupt { .. }
            ) && let Some(limit) = imports.code_fetch_limit
            {
                code_fetch::refresh(code, limit);
            }
            abi::debug_after_retirement(code, remaining + 2, imports.detection, false);
        }
        code.br(1).end();
    }
    if let Some(fault) = block.context_fault() {
        code.local_get(EIP)
            .i32_const(fault.pc.0 as i32)
            .i32_eq()
            .if_(BlockType::Empty);
        abi::safepoint(code, 2);
        code_fetch::check_context(code, 2);
        code_fetch::refresh(
            code,
            imports
                .code_fetch_limit
                .expect("context fault requires a bound module"),
        );
        code_fetch::check_span(
            code,
            fault.pc.0,
            fault.attempted_length,
            imports
                .code_fetch_limit
                .expect("context fault requires a bound module"),
            2,
        );
        code.i32_const(3).local_set(REASON).br(2).end();
    }
}
