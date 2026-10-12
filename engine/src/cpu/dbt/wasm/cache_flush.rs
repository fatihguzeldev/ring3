use wasm_encoder::InstructionSink;

use super::memory;
use crate::cpu::x86::{
    SegmentAccess,
    ir::{EffectiveAddress, SmallWidth},
};

pub(super) fn instruction(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: memory::Imports,
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
