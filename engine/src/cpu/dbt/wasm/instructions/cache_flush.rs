use crate::cpu::dbt::wasm::imports::Imports;
use crate::cpu::{
    dbt::wasm::instructions::memory,
    x86::{
        SegmentAccess,
        ir::{EffectiveAddress, SmallWidth},
    },
};
use wasm_encoder::InstructionSink;
pub(in crate::cpu::dbt::wasm) fn instruction(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: Imports,
    exit_depth: u32,
) {
    // coherent RAM and synchronous executable-write invalidation need only the checked byte access.
    memory::checked_address(
        code,
        address,
        1,
        SegmentAccess::CacheFlush,
        imports,
        exit_depth,
    );
    memory::load_narrow_at_address(code, SmallWidth::Byte, imports, exit_depth);
    code.drop();
}
