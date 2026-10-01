use wasm_encoder::{BlockType, InstructionSink, MemArg};

use crate::abi::{
    ABI_VERSION, X86_INTEGER_PROFILE,
    x86::{
        ACCESS_LENGTH_OFFSET, ACCESS_OFFSET, DETAIL_OFFSET, EFLAGS_OFFSET, EIP_OFFSET, EXIT_SIZE,
        FAULT_ADDRESS_OFFSET, REASON_OFFSET, REGISTERS_OFFSET, RETIRED_OFFSET, STATE_SIZE,
    },
};

use super::locals::{
    BUDGET, CANCEL_PTR, DETAIL, EIP, EXIT_PTR, FAULT_ACCESS, FAULT_ADDRESS, FAULT_LENGTH, FLAGS,
    MEMORY_BYTES, REASON, RETIRED, STATE_PTR,
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

    for (offset, expected) in [
        (0, u32::from_le_bytes(*b"R3ST")),
        (4, header_version()),
        (8, STATE_SIZE as u32),
        (12, 0),
    ] {
        load(sink, STATE_PTR, offset);
        sink.i32_const(expected as i32).i32_ne();
        fail_if(sink, 2);
    }
    load(sink, STATE_PTR, EFLAGS_OFFSET);
    sink.local_set(FLAGS)
        .local_get(FLAGS)
        .i32_const(2)
        .i32_and()
        .i32_eqz()
        .local_get(FLAGS)
        .i32_const(!0xcd7)
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
}

pub(super) fn flush(sink: &mut InstructionSink<'_>, memory: bool) {
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

    for (offset, value) in [
        (0, u32::from_le_bytes(*b"R3EX")),
        (
            4,
            if memory {
                2 | (u32::from(X86_INTEGER_PROFILE) << 16)
            } else {
                header_version()
            },
        ),
        (8, EXIT_SIZE as u32),
        (12, 0),
        (DETAIL_OFFSET, 0),
        (FAULT_ADDRESS_OFFSET, 0),
        (ACCESS_OFFSET, 0),
        (ACCESS_LENGTH_OFFSET, 0),
    ] {
        sink.local_get(EXIT_PTR)
            .i32_const(value as i32)
            .i32_store(memarg(offset));
    }
    store_local(sink, EXIT_PTR, REASON_OFFSET, REASON);
    store_local(sink, EXIT_PTR, RETIRED_OFFSET, RETIRED);
    if memory {
        for (offset, local) in [
            (DETAIL_OFFSET, DETAIL),
            (FAULT_ADDRESS_OFFSET, FAULT_ADDRESS),
            (ACCESS_OFFSET, FAULT_ACCESS),
            (ACCESS_LENGTH_OFFSET, FAULT_LENGTH),
        ] {
            store_local(sink, EXIT_PTR, offset, local);
        }
    }
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
