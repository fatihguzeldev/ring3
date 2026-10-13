use super::{
    cpu_result,
    locals::*,
    memory::{self, Imports, StoreImport},
};
use crate::{
    abi::{
        arena::{TRANSFER_OFFSET, X87_OFFSET},
        x86::{X87_STATUS_OFFSET, X87_TAG_OFFSET},
    },
    cpu::x86::simd::{FloatKind, SimdKind, SimdOperand, SimdOperation, SimdRegister},
};
use wasm_encoder::{InstructionSink, MemArg};

mod direct;

#[cfg(test)]
pub(super) fn integer_lanes(
    code: &mut InstructionSink<'_>,
    operation: crate::cpu::x86::simd::IntegerLaneOperation,
    scratch: [u32; 2],
) {
    direct::integer_lanes(code, operation, scratch);
}

#[cfg(test)]
pub(super) fn rearrange_value(
    code: &mut InstructionSink<'_>,
    operation: crate::cpu::x86::simd::RearrangeOperation,
    sources: [u32; 3],
) {
    direct::rearrange_value(code, operation, sources);
}

#[cfg(test)]
pub(super) fn shift_value(
    code: &mut InstructionSink<'_>,
    operation: crate::cpu::x86::simd::ShiftOperation,
    immediate: Option<u8>,
    sources: [u32; 2],
) {
    direct::shift_value(code, operation, immediate, sources);
}

pub(super) fn direct_hotpath(operation: SimdOperation) -> bool {
    direct::accepts(operation)
}

fn gpr(code: &mut InstructionSink<'_>, operand: SimdOperand) {
    if let SimdOperand::Register(SimdRegister::Gpr(value)) = operand {
        code.local_get(register(value));
    } else {
        code.i32_const(0);
    }
}
fn payload(code: &mut InstructionSink<'_>, offset: usize) {
    code.local_get(STATE_PTR).i32_load(MemArg {
        offset: (TRANSFER_OFFSET + 512 + offset) as u64,
        align: 2,
        memory_index: 0,
    });
}
fn mark_mmx(code: &mut InstructionSink<'_>) {
    code.local_get(STATE_PTR).i32_const(0).i32_store16(MemArg {
        offset: (X87_OFFSET + X87_TAG_OFFSET) as u64,
        align: 1,
        memory_index: 0,
    });
    code.local_get(STATE_PTR)
        .local_get(STATE_PTR)
        .i32_load16_u(MemArg {
            offset: (X87_OFFSET + X87_STATUS_OFFSET) as u64,
            align: 1,
            memory_index: 0,
        })
        .i32_const(!0x3800)
        .i32_and()
        .i32_store16(MemArg {
            offset: (X87_OFFSET + X87_STATUS_OFFSET) as u64,
            align: 1,
            memory_index: 0,
        });
}

pub(super) fn instruction(
    code: &mut InstructionSink<'_>,
    operation: SimdOperation,
    imports: Imports,
    exit_depth: u32,
) -> bool {
    if operation.uses_mmx() || operation.kind == SimdKind::EmptyMmx {
        super::x87::pending(code, exit_depth);
    }
    if matches!(operation.kind, SimdKind::Fence | SimdKind::Prefetch) {
        return false;
    }
    if direct_hotpath(operation) {
        return direct::instruction(code, operation, imports, exit_depth);
    }
    let alignment = if operation.aligned {
        if operation.vector_bytes == 8 { 8 } else { 16 }
    } else {
        1
    };
    if let Some(address) = operation.implicit_address {
        memory::address_value(code, address);
        code.local_set(ADDRESS);
    }
    if let SimdOperand::Memory(address) = if operation.reads_memory() {
        operation.source
    } else {
        operation.destination
    } {
        memory::checked_address(
            code,
            address,
            u32::from(operation.memory_bytes),
            if operation.writes_memory() {
                crate::cpu::x86::SegmentAccess::Write
            } else {
                crate::cpu::x86::SegmentAccess::Read
            },
            imports,
            exit_depth,
        );
    }
    if operation.reads_memory() {
        code.local_get(ADDRESS)
            .i32_const(i32::from(operation.memory_bytes))
            .i32_const(alignment)
            .call(imports.cpu_read.expect("SIMD memory read import"))
            .local_set(HELPER_STATUS);
        cpu_result::validate(
            code,
            u32::from(operation.memory_bytes),
            1,
            false,
            exit_depth,
        );
    }
    let (descriptor, operands, immediate) = operation.descriptor();
    let execute = imports
        .simd_execute
        .expect("SIMD execution import")
        .emit_binding(code);
    code.i32_const(descriptor as i32)
        .i32_const(operands as i32)
        .i32_const(immediate as i32);
    gpr(code, operation.source);
    gpr(code, operation.destination);
    code.local_get(FLAGS);
    code.call(execute).local_set(HELPER_STATUS);
    cpu_result::validate(code, 0, 0, false, exit_depth);
    memory::reset_checks(code);
    memory::expect_field(code, 20, 0);
    memory::protocol_if_bad(code);
    memory::exit_if_failed(code, exit_depth);
    let compare_flags = matches!(
        operation.kind,
        SimdKind::Float(FloatKind::CompareFlags | FloatKind::CompareFlagsQuiet)
    );
    payload(code, 4);
    if compare_flags {
        code.local_set(RESULT);
        memory::reset_checks(code);
        // a completed simd comparison has one of four relations; exceptions exit above.
        code.local_get(RESULT)
            .i32_const(0x8d5)
            .i32_and()
            .i32_eqz()
            .i32_eqz();
        for relation in [1, 0x40, 0x45] {
            code.local_get(RESULT)
                .i32_const(0x8d5)
                .i32_and()
                .i32_const(relation)
                .i32_ne()
                .i32_and();
        }
        memory::add_check(code);
        memory::protocol_if_bad(code);
        memory::exit_if_failed(code, exit_depth);
        code.local_get(RESULT);
    }
    cpu_result::flags(code, if compare_flags { 0x8d5 } else { 0 }, exit_depth);
    if operation.writes_memory() {
        let import = if operation.kind == SimdKind::SaveExtended {
            imports.cpu_fxsave
        } else if operation.kind == SimdKind::MaskedStore {
            imports.cpu_mask_write
        } else {
            imports.cpu_write
        }
        .expect("SIMD memory write import");
        let index = match import {
            StoreImport::Replacement(index) => index,
            StoreImport::Resident { index, key, id } => {
                code.i32_const(key as u32 as i32)
                    .i32_const((key >> 32) as u32 as i32)
                    .i32_const(id as u32 as i32)
                    .i32_const((id >> 32) as u32 as i32);
                index
            }
        };
        if operation.kind == SimdKind::MaskedStore {
            payload(code, 0);
            code.local_set(RESUME);
            code.local_get(RESUME).if_(wasm_encoder::BlockType::Empty);
            super::segments::access(
                code,
                operation
                    .implicit_address
                    .expect("MASKMOV has an implicit address")
                    .segment
                    .unwrap_or(crate::cpu::x86::SegmentRegister::Ds),
                exit_depth + 1,
            );
            code.end();
        }
        code.local_get(ADDRESS);
        if operation.kind == SimdKind::MaskedStore {
            code.i32_const(i32::from(operation.memory_bytes))
                .local_get(RESUME);
        } else if operation.kind != SimdKind::SaveExtended {
            code.i32_const(i32::from(operation.memory_bytes))
                .i32_const(alignment);
        }
        code.call(index).local_set(HELPER_STATUS);
        if operation.kind == SimdKind::MaskedStore {
            cpu_result::validate_masked(
                code,
                u32::from(operation.memory_bytes),
                RESUME,
                exit_depth,
            );
        } else {
            cpu_result::validate(code, u32::from(operation.memory_bytes), 2, true, exit_depth);
        }
        if operation.uses_mmx() {
            mark_mmx(code);
        }
    }
    if let SimdOperand::Register(SimdRegister::Gpr(destination)) = operation.destination {
        payload(code, 0);
        code.local_set(register(destination));
    }
    operation.writes_memory()
}

pub(super) fn preflight(code: &mut InstructionSink<'_>) {
    use crate::abi::{
        ABI_VERSION, X86_INTEGER_PROFILE,
        arena::SIMD_OFFSET,
        x86::{SIMD_MXCSR_OFFSET, SIMD_SIZE},
    };
    use wasm_encoder::BlockType;
    code.local_get(STATE_PTR)
        .i64_extend_i32_u()
        .i64_const((SIMD_OFFSET + SIMD_SIZE) as i64)
        .i64_add()
        .local_get(MEMORY_BYTES)
        .i64_gt_u();
    code.if_(BlockType::Empty).i32_const(1).return_().end();
    for (pointer, size) in [
        (STATE_PTR, crate::abi::x86::STATE_SIZE),
        (EXIT_PTR, crate::abi::x86::EXIT_SIZE),
        (CANCEL_PTR, 4),
    ] {
        code.local_get(STATE_PTR)
            .i64_extend_i32_u()
            .i64_const(SIMD_OFFSET as i64)
            .i64_add()
            .local_get(pointer)
            .i64_extend_i32_u()
            .i64_const(size as i64)
            .i64_add()
            .i64_lt_u();
        code.local_get(pointer)
            .i64_extend_i32_u()
            .local_get(STATE_PTR)
            .i64_extend_i32_u()
            .i64_const((SIMD_OFFSET + SIMD_SIZE) as i64)
            .i64_add()
            .i64_lt_u()
            .i32_and();
        code.if_(BlockType::Empty).i32_const(1).return_().end();
    }
    for (offset, expected) in [
        (0, u32::from_le_bytes(*b"R3SV")),
        (
            4,
            u32::from(ABI_VERSION) | (u32::from(X86_INTEGER_PROFILE) << 16),
        ),
        (8, SIMD_SIZE as u32),
        (12, 0),
        (20, 0),
        (24, 0),
        (28, 0),
    ] {
        code.local_get(STATE_PTR)
            .i32_load(MemArg {
                offset: (SIMD_OFFSET + offset) as u64,
                align: 2,
                memory_index: 0,
            })
            .i32_const(expected as i32)
            .i32_ne()
            .if_(BlockType::Empty)
            .i32_const(2)
            .return_()
            .end();
    }
    code.local_get(STATE_PTR)
        .i32_load(MemArg {
            offset: (SIMD_OFFSET + SIMD_MXCSR_OFFSET) as u64,
            align: 2,
            memory_index: 0,
        })
        .i32_const(!0xffff)
        .i32_and()
        .if_(BlockType::Empty)
        .i32_const(2)
        .return_()
        .end();
    super::x87::preflight(code);
}
