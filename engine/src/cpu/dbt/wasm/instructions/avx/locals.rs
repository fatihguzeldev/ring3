#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::dbt::wasm) struct AvxLocals {
    pub operand: u32,
    pub control: u32,
    pub saved: u32,
    pub vectors: [u32; 6],
}

impl AvxLocals {
    pub(in crate::cpu::dbt::wasm) fn new(first: u32) -> Self {
        Self {
            operand: first,
            control: first + 1,
            saved: first + 2,
            vectors: std::array::from_fn(|index| first + 3 + index as u32),
        }
    }
}
