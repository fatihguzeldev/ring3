mod families;
mod layout;

use super::*;
fn state() -> ContextState32 {
    let mut segments = SegmentContext::default();
    segments.state.bases[4] = 0x7ffd0000;
    ContextState32 {
        state: State32 {
            registers: [
                0x11111111, 0x22222222, 0x33333333, 0x44444444, 0x55555555, 0x66666666, 0x77777777,
                0x88888888,
            ],
            eip: 0x12345678,
            eflags: 0x202,
        },
        segments,
        x87: X87State {
            control: 0x37f,
            status: 0x1800,
            tag: 0,
            opcode: 0x123,
            instruction_pointer: 0x10203040,
            data_pointer: 0x50607080,
            code_selector: 0x1b,
            data_selector: 0x23,
            registers: std::array::from_fn(|index| {
                [index as u8, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f]
            }),
        },
        simd: SimdState {
            mxcsr: 0x1f80,
            xmm: std::array::from_fn(|index| [index as u8 + 1; 16]),
        },
        debug: [0x1000, 0x2000, 0x3000, 0x4000, 0xffff0ff0, 0x400],
    }
}
fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn write(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
