use wasm_encoder::InstructionSink;

use super::memory;
use crate::cpu::x86::ir::{EffectiveAddress, SmallWidth};

pub(super) fn instruction(
    code: &mut InstructionSink<'_>,
    address: EffectiveAddress,
    imports: memory::Imports,
    exit_depth: u32,
) {
    // coherent RAM and synchronous executable-write invalidation need only the checked byte access.
    memory::load_narrow_value(code, address, SmallWidth::Byte, imports, exit_depth);
    code.drop();
}
