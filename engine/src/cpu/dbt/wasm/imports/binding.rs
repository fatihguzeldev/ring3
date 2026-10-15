use wasm_encoder::InstructionSink;

#[derive(Clone, Copy)]
pub(in crate::cpu::dbt::wasm) enum HelperBinding {
    Replacement(u32),
    Resident { index: u32, key: u64, id: u64 },
}

impl HelperBinding {
    pub(in crate::cpu::dbt::wasm) fn emit_binding(self, code: &mut InstructionSink<'_>) -> u32 {
        match self {
            Self::Replacement(index) => index,
            Self::Resident { index, key, id } => {
                code.i32_const(key as u32 as i32)
                    .i32_const((key >> 32) as u32 as i32)
                    .i32_const(id as u32 as i32)
                    .i32_const((id >> 32) as u32 as i32);
                index
            }
        }
    }
}
