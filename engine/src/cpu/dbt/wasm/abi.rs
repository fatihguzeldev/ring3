use wasm_encoder::{BlockType, InstructionSink, MemArg};

use crate::abi::{
    ABI_VERSION, X86_INTEGER_PROFILE,
    x86::{
        ACCESS_LENGTH_OFFSET, ACCESS_OFFSET, DETAIL_OFFSET, EFLAGS_OFFSET, EIP_OFFSET, EXIT_SIZE,
        EXIT_VERSION_5, FAULT_ADDRESS_OFFSET, REASON_OFFSET, REGISTERS_OFFSET, RETIRED_OFFSET,
        STATE_METADATA_OFFSET, STATE_SIZE,
    },
};

use super::locals::{
    BUDGET, CANCEL_PTR, DEBUG_SHADOW, DETAIL, EIP, EXIT_PTR, FAULT_ACCESS, FAULT_ADDRESS,
    FAULT_LENGTH, FLAGS, MEMORY_BYTES, REASON, RETIRED, SNAPSHOT_SHADOW, SNAPSHOT_TF, STATE_PTR,
};

pub(super) fn preflight(sink: &mut InstructionSink<'_>) {
    sink.memory_size(0)
        .i64_extend_i32_u()
        .i64_const(16)
        .i64_shl()
        .local_set(MEMORY_BYTES);

    let ranges = [
        (STATE_PTR, STATE_SIZE as u64),
        (EXIT_PTR, EXIT_SIZE as u64),
        (CANCEL_PTR, 4),
    ];
    for &(pointer, size) in &ranges {
        range_end(sink, pointer, size);
        sink.local_get(MEMORY_BYTES).i64_gt_u();
        fail_if(sink, 1);
    }
    for (index, &(pointer, size)) in ranges.iter().enumerate() {
        for &(other, other_size) in &ranges[..index] {
            unsigned_pointer(sink, pointer);
            range_end(sink, other, other_size);
            sink.i64_lt_u();
            unsigned_pointer(sink, other);
            range_end(sink, pointer, size);
            sink.i64_lt_u().i32_and();
            fail_if(sink, 1);
        }
    }

    for (offset, expected) in [(0, u32::from_le_bytes(*b"R3ST")), (8, STATE_SIZE as u32)] {
        load(sink, STATE_PTR, offset);
        sink.i32_const(expected as i32).i32_ne();
        fail_if(sink, 2);
    }
    load(sink, STATE_PTR, 4);
    sink.i32_const(header_version() as i32).i32_ne();
    load(sink, STATE_PTR, 4);
    sink.i32_const(2 | (i32::from(X86_INTEGER_PROFILE) << 16))
        .i32_ne()
        .i32_and();
    load(sink, STATE_PTR, 4);
    sink.i32_const(3 | (i32::from(X86_INTEGER_PROFILE) << 16))
        .i32_ne()
        .i32_and();
    fail_if(sink, 2);
    load(sink, STATE_PTR, STATE_METADATA_OFFSET);
    load(sink, STATE_PTR, 4);
    sink.i32_const(header_version() as i32).i32_ne().i32_gt_u();
    fail_if(sink, 2);
    load(sink, STATE_PTR, EFLAGS_OFFSET);
    sink.local_set(FLAGS)
        .local_get(FLAGS)
        .i32_const(2)
        .i32_and()
        .i32_eqz()
        .local_get(FLAGS);
    load(sink, STATE_PTR, 4);
    sink.i32_const(header_version() as i32)
        .i32_eq()
        .if_(BlockType::Result(wasm_encoder::ValType::I32))
        .i32_const(!crate::abi::x86::LEGACY_FLAGS_MASK as i32)
        .else_();
    load(sink, STATE_PTR, 4);
    sink.i32_const(2 | (i32::from(X86_INTEGER_PROFILE) << 16))
        .i32_eq()
        .if_(BlockType::Result(wasm_encoder::ValType::I32))
        .i32_const(!crate::abi::x86::USER_FLAGS_MASK as i32)
        .else_()
        .i32_const(!crate::abi::x86::USER_FLAGS_MASK_V3 as i32)
        .end()
        .end()
        .i32_and()
        .i32_or();
    fail_if(sink, 2);
}

pub(super) fn load_state(sink: &mut InstructionSink<'_>) {
    for index in 0..8 {
        load(sink, STATE_PTR, REGISTERS_OFFSET + index * 4);
        sink.local_set(4 + index as u32);
    }
    load(sink, STATE_PTR, EIP_OFFSET);
    sink.local_set(EIP);
    load(sink, STATE_PTR, EFLAGS_OFFSET);
    sink.local_set(FLAGS);
    load(sink, STATE_PTR, STATE_METADATA_OFFSET);
    sink.local_set(DEBUG_SHADOW);
}

pub(super) fn work_preflight(sink: &mut InstructionSink<'_>) {
    use crate::abi::arena::{WORK_OFFSET, WORK_SIZE};
    range_end(sink, STATE_PTR, (WORK_OFFSET + WORK_SIZE) as u64);
    sink.local_get(MEMORY_BYTES).i64_gt_u();
    fail_if(sink, 1);
    for (pointer, size) in [(EXIT_PTR, EXIT_SIZE as u64), (CANCEL_PTR, 4)] {
        range_end(sink, STATE_PTR, WORK_OFFSET as u64);
        range_end(sink, pointer, size);
        sink.i64_lt_u();
        unsigned_pointer(sink, pointer);
        range_end(sink, STATE_PTR, (WORK_OFFSET + WORK_SIZE) as u64);
        sink.i64_lt_u().i32_and();
        fail_if(sink, 1);
    }
    for (offset, expected) in [
        (0, u32::from_le_bytes(*b"R3WK")),
        (4, header_version()),
        (8, WORK_SIZE as u32),
        (12, 0),
        (20, 0),
    ] {
        load(sink, STATE_PTR, WORK_OFFSET + offset);
        sink.i32_const(expected as i32).i32_ne();
        fail_if(sink, 2);
    }
}

pub(super) fn flush(
    sink: &mut InstructionSink<'_>,
    memory: bool,
    gates: bool,
    arithmetic: bool,
    cpu: bool,
    xstate: bool,
) {
    for index in 0..8 {
        store_local(
            sink,
            STATE_PTR,
            REGISTERS_OFFSET + index * 4,
            4 + index as u32,
        );
    }
    store_local(sink, STATE_PTR, EIP_OFFSET, EIP);
    store_local(sink, STATE_PTR, EFLAGS_OFFSET, FLAGS);
    store_local(sink, STATE_PTR, STATE_METADATA_OFFSET, DEBUG_SHADOW);
    load(sink, STATE_PTR, 4);
    sink.i32_const(3 | (i32::from(X86_INTEGER_PROFILE) << 16))
        .i32_eq()
        .local_get(FLAGS)
        .i32_const(0x10000)
        .i32_and()
        .i32_or()
        .if_(BlockType::Empty)
        .local_get(STATE_PTR)
        .i32_const(3 | (i32::from(X86_INTEGER_PROFILE) << 16))
        .i32_store(memarg(4))
        .else_();
    sink.local_get(FLAGS)
        .i32_const(!crate::abi::x86::LEGACY_FLAGS_MASK as i32)
        .i32_and()
        .local_get(DEBUG_SHADOW)
        .i32_or()
        .if_(BlockType::Empty)
        .local_get(STATE_PTR)
        .i32_const(2 | (i32::from(X86_INTEGER_PROFILE) << 16))
        .i32_store(memarg(4))
        .end()
        .end();

    let detail_fields = [
        (DETAIL_OFFSET, DETAIL),
        (FAULT_ADDRESS_OFFSET, FAULT_ADDRESS),
        (ACCESS_OFFSET, FAULT_ACCESS),
        (ACCESS_LENGTH_OFFSET, FAULT_LENGTH),
    ];
    for (offset, value) in [
        (0, u32::from_le_bytes(*b"R3EX")),
        (
            4,
            if xstate {
                let version = if crate::cpu::x86::capabilities::AVX_ENABLED {
                    9
                } else {
                    8
                };
                version | (u32::from(X86_INTEGER_PROFILE) << 16)
            } else if cpu {
                6 | (u32::from(X86_INTEGER_PROFILE) << 16)
            } else if arithmetic {
                u32::from(EXIT_VERSION_5) | (u32::from(X86_INTEGER_PROFILE) << 16)
            } else if gates {
                3 | (u32::from(X86_INTEGER_PROFILE) << 16)
            } else if memory {
                2 | (u32::from(X86_INTEGER_PROFILE) << 16)
            } else {
                header_version()
            },
        ),
        (8, EXIT_SIZE as u32),
        (12, 0),
    ] {
        sink.local_get(EXIT_PTR)
            .i32_const(value as i32)
            .i32_store(memarg(offset));
    }
    if !(memory || gates || cpu) {
        for (offset, _) in detail_fields {
            sink.local_get(EXIT_PTR)
                .i32_const(0)
                .i32_store(memarg(offset));
        }
    }
    if !cpu {
        sink.local_get(REASON)
            .i32_const(11)
            .i32_ge_u()
            .if_(BlockType::Empty)
            .local_get(EXIT_PTR)
            .i32_const(6 | (i32::from(X86_INTEGER_PROFILE) << 16))
            .i32_store(memarg(4))
            .end();
    }
    store_local(sink, EXIT_PTR, REASON_OFFSET, REASON);
    store_local(sink, EXIT_PTR, RETIRED_OFFSET, RETIRED);
    if memory || gates || cpu {
        for (offset, local) in detail_fields {
            store_local(sink, EXIT_PTR, offset, local);
        }
    }
    if xstate {
        return;
    }
    sink.local_get(REASON)
        .i32_const(16)
        .i32_ge_u()
        .if_(BlockType::Empty)
        .local_get(EXIT_PTR)
        .i32_const(7 | (i32::from(X86_INTEGER_PROFILE) << 16))
        .i32_store(memarg(4));
    if !(memory || gates || cpu) {
        for (offset, local) in detail_fields {
            store_local(sink, EXIT_PTR, offset, local);
        }
    }
    sink.end();
}

pub(super) fn interrupt_return_precheck(sink: &mut InstructionSink<'_>, exit_depth: u32) {
    // task return requires trusted tss linkage, outside the same-CPL return owner.
    sink.local_get(FLAGS)
        .i32_const(0x4000)
        .i32_and()
        .if_(BlockType::Empty)
        .i32_const(4)
        .local_set(REASON)
        .i32_const(4)
        .local_set(DETAIL);
    for local in [FAULT_ADDRESS, FAULT_ACCESS, FAULT_LENGTH] {
        sink.i32_const(0).local_set(local);
    }
    sink.br(exit_depth + 1).end();
}

pub(super) fn begin_instruction(sink: &mut InstructionSink<'_>) {
    sink.local_get(FLAGS)
        .i32_const(0x100)
        .i32_and()
        .local_set(SNAPSHOT_TF)
        .local_get(DEBUG_SHADOW)
        .local_set(SNAPSHOT_SHADOW)
        .i32_const(0)
        .local_set(DEBUG_SHADOW)
        .local_get(FLAGS)
        .i32_const(!0x10000)
        .i32_and()
        .local_set(FLAGS);
}

pub(super) fn inhibit_debug_after_ss_load(sink: &mut InstructionSink<'_>) {
    // consecutive ss loads do not extend the first instruction's inhibition.
    sink.local_get(SNAPSHOT_SHADOW)
        .i32_eqz()
        .if_(BlockType::Empty)
        .i32_const(1)
        .local_set(DEBUG_SHADOW)
        .i32_const(0)
        .local_set(SNAPSHOT_TF)
        .end();
}

pub(super) fn debug_after_retirement(sink: &mut InstructionSink<'_>, exit_depth: u32) {
    sink.local_get(SNAPSHOT_TF)
        .if_(BlockType::Empty)
        .i32_const(16)
        .local_set(REASON);
    for local in [DETAIL, FAULT_ADDRESS, FAULT_ACCESS, FAULT_LENGTH] {
        sink.i32_const(0).local_set(local);
    }
    sink.br(exit_depth + 1).end();
}

pub(super) fn safepoint(sink: &mut InstructionSink<'_>, exit_depth: u32) {
    load(sink, CANCEL_PTR, 0);
    sink.if_(BlockType::Empty)
        .i32_const(2)
        .local_set(REASON)
        .br(exit_depth + 1)
        .end();
    sink.local_get(BUDGET)
        .i32_eqz()
        .if_(BlockType::Empty)
        .i32_const(1)
        .local_set(REASON)
        .br(exit_depth + 1)
        .end();
}

fn unsigned_pointer(sink: &mut InstructionSink<'_>, pointer: u32) {
    sink.local_get(pointer).i64_extend_i32_u();
}

fn range_end(sink: &mut InstructionSink<'_>, pointer: u32, size: u64) {
    unsigned_pointer(sink, pointer);
    sink.i64_const(size as i64).i64_add();
}

fn fail_if(sink: &mut InstructionSink<'_>, status: i32) {
    sink.if_(BlockType::Empty).i32_const(status).return_().end();
}

fn header_version() -> u32 {
    u32::from(ABI_VERSION) | (u32::from(X86_INTEGER_PROFILE) << 16)
}

fn load(sink: &mut InstructionSink<'_>, pointer: u32, offset: usize) {
    sink.local_get(pointer).i32_load(memarg(offset));
}

fn store_local(sink: &mut InstructionSink<'_>, pointer: u32, offset: usize, local: u32) {
    sink.local_get(pointer)
        .local_get(local)
        .i32_store(memarg(offset));
}

fn memarg(offset: usize) -> MemArg {
    MemArg {
        offset: offset as u64,
        align: 2,
        memory_index: 0,
    }
}
