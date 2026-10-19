use crate::cpu::{
    dbt::{
        region::CompiledBlock,
        wasm::{
            EmbeddedBinding,
            imports::{ImportNeeds, Imports},
        },
    },
    x86::ir::Operation,
};
pub(in crate::cpu::dbt::wasm) struct ModuleRequirements {
    pub(in crate::cpu::dbt::wasm) imports: ImportNeeds,
    pub(in crate::cpu::dbt::wasm) modify: [bool; 3],
    pub(in crate::cpu::dbt::wasm) paging_configured: bool,
    pub(in crate::cpu::dbt::wasm) detects_exceptions: bool,
    pub(in crate::cpu::dbt::wasm) needs_fetch_paging: bool,
    pub(in crate::cpu::dbt::wasm) has_stores: bool,
    pub(in crate::cpu::dbt::wasm) has_reads: bool,
    pub(in crate::cpu::dbt::wasm) has_avx: bool,
    pub(in crate::cpu::dbt::wasm) has_simd: bool,
    pub(in crate::cpu::dbt::wasm) has_xstate: bool,
    pub(in crate::cpu::dbt::wasm) has_cpu: bool,
    pub(in crate::cpu::dbt::wasm) has_memory: bool,
    pub(in crate::cpu::dbt::wasm) has_gates: bool,
    pub(in crate::cpu::dbt::wasm) has_arithmetic: bool,
    pub(in crate::cpu::dbt::wasm) has_x87: bool,
    pub(in crate::cpu::dbt::wasm) has_repeat: bool,
    pub(in crate::cpu::dbt::wasm) has_segments: bool,
    pub(in crate::cpu::dbt::wasm) has_segment_check: bool,
    pub(in crate::cpu::dbt::wasm) has_zero_span_validator: bool,
    pub(in crate::cpu::dbt::wasm) has_x87_read32_validator: bool,
    pub(in crate::cpu::dbt::wasm) has_read32_validator: bool,
    pub(in crate::cpu::dbt::wasm) upper_xstate: bool,
}

impl ModuleRequirements {
    pub(in crate::cpu::dbt::wasm) fn new(
        blocks: &[CompiledBlock],
        binding: Option<EmbeddedBinding>,
    ) -> Self {
        let paging_configured = binding.is_some();
        let detects_exceptions = binding.is_some();
        let needs_fetch_paging = blocks.iter().any(|block| {
            block.context_fault().is_some_and(|fault| {
                fault.kind == crate::cpu::x86::decode::CapturedFetchFaultKind32::Paging
            })
        });
        let imports = Imports::needed(blocks);
        let modify = if paging_configured {
            crate::cpu::dbt::wasm::imports::read_modify_needs(blocks)
        } else {
            [false; 3]
        };
        let has_stores = imports.store
            || imports.store8
            || imports.store16
            || imports.exchange
            || imports.exchange8
            || imports.exchange16
            || imports.exchange_add32
            || imports.exchange_add8
            || imports.exchange_add16
            || imports.bit_test_set32
            || imports.bit_test_reset32
            || imports.atomic_or_u32
            || imports.atomic_and_u32
            || imports.atomic_xor_u32;
        let has_reads = imports.read || imports.read8 || imports.read16;
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
                                | crate::cpu::x86::ControlOperation::Sysenter
                        ) | Operation::SoftwareInterrupt { .. }
                            | Operation::XState(_)
                    )
                });
        let has_cpu = has_simd
            || has_control_fault
            || imports.stack_execute
            || imports.cpu_integer
            || imports.cpu_read
            || imports.cpu_write
            || imports.x87_op
            || imports.simd_execute
            || imports.avx_execute
            || imports.cpu_fxsave
            || imports.cpu_xsave
            || imports.cpu_xrstor
            || imports.cpu_mask_write
            || imports.segment_load
            || imports.segment_load_far
            || imports.far_jump
            || imports.far_call
            || imports.far_return;
        let has_memory = has_cpu
            || has_reads
            || has_stores
            || imports.compare_exchange32
            || imports.compare_exchange8
            || imports.compare_exchange16;
        let has_gates = blocks.iter().any(|block| block.gate().is_some());
        let has_arithmetic =
            blocks
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
        let upper_xstate =
            crate::cpu::x86::capabilities::AVX_ENABLED && (imports.cpu_xsave || imports.cpu_xrstor);
        // one site grows when it owns the shared failure body; two sites already amortize it.
        let has_read32_validator = paging_configured && imports.read32_result_sites >= 2;
        // waiting instructions emit a second operation site for pending x87 exceptions.
        let x87_read32_sites: usize = blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .map(|instruction| match instruction.operation() {
                Operation::X87 {
                    operation, waiting, ..
                } if operation.read_size() == 4 && operation.write_size() == 0 => {
                    1 + usize::from(*waiting)
                }
                _ => 0,
            })
            .sum();
        let has_x87_read32_validator = paging_configured && x87_read32_sites >= 2;
        // register x87 operations emit the same ordinary result checks at each helper site.
        // other modules retain their current layout without an unused local function.
        let has_zero_span_validator = has_x87
            && paging_configured
            && blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .any(|instruction| {
                    matches!(
                        instruction.operation(),
                        Operation::X87 { operation, address: None, .. }
                            if operation.read_size() == 0 && operation.write_size() == 0
                    )
                });
        Self {
            imports,
            modify,
            paging_configured,
            detects_exceptions,
            needs_fetch_paging,
            has_stores,
            has_reads,
            has_avx,
            has_simd,
            has_xstate,
            has_cpu,
            has_memory,
            has_gates,
            has_arithmetic,
            has_x87,
            has_repeat,
            has_segments,
            has_segment_check,
            has_zero_span_validator,
            has_x87_read32_validator,
            has_read32_validator,
            upper_xstate,
        }
    }
}
