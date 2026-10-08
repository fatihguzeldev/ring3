#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct X87State {
    pub control: u16,
    pub status: u16,
    pub tag: u16,
    pub opcode: u16,
    pub instruction_pointer: u32,
    pub data_pointer: u32,
    pub code_selector: u16,
    pub data_selector: u16,
    pub registers: [[u8; 10]; 8],
}

impl Default for X87State {
    fn default() -> Self {
        Self {
            control: 0x037f,
            status: 0,
            tag: 0xffff,
            opcode: 0,
            instruction_pointer: 0,
            data_pointer: 0,
            code_selector: 0,
            data_selector: 0,
            registers: [[0; 10]; 8],
        }
    }
}
