use super::{cpu_result, locals::*, memory};
use crate::{
    abi::arena::TRANSFER_OFFSET,
    cpu::x86::{SegmentAccess, ir::EffectiveAddress},
};
use wasm_encoder::{InstructionSink, MemArg};

pub(super) fn store_descriptor(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    limit: u16,
    base: u32,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::checked_address(code, address, 6, SegmentAccess::Write, imports, exit_depth);
    code.local_get(STATE_PTR)
        .i32_const(i32::from(limit))
        .i32_store16(transfer(0));
    code.local_get(STATE_PTR)
        .i32_const(base as i32)
        .i32_store(transfer(2));
    let index = imports
        .cpu_write
        .expect("six-byte system descriptor store import")
        .emit_binding(code);
    code.local_get(ADDRESS)
        .i32_const(6)
        .i32_const(1)
        .call(index)
        .local_set(HELPER_STATUS);
    cpu_result::validate(
        code,
        6,
        2,
        true,
        exit_depth,
        cpu_result::ResultContext::from(imports),
    );
}

fn transfer(offset: usize) -> MemArg {
    MemArg {
        offset: (TRANSFER_OFFSET + offset) as u64,
        align: 0,
        memory_index: 0,
    }
}
