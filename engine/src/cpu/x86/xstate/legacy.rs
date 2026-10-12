use crate::cpu::x86::{X87State, simd::SimdState};

pub(in crate::cpu::x86) fn save_x87(x87: &X87State, output: &mut [u8]) {
    output[..24].fill(0);
    output[32..160].fill(0);
    output[0..2].copy_from_slice(&x87.control.to_le_bytes());
    output[2..4].copy_from_slice(&x87.status.to_le_bytes());
    output[4] = (0..8).fold(0, |tag, index| {
        tag | u8::from((x87.tag >> (index * 2)) & 3 != 3) << index
    });
    output[6..8].copy_from_slice(&x87.opcode.to_le_bytes());
    output[8..12].copy_from_slice(&x87.instruction_pointer.to_le_bytes());
    output[12..14].copy_from_slice(&x87.code_selector.to_le_bytes());
    output[16..20].copy_from_slice(&x87.data_pointer.to_le_bytes());
    output[20..22].copy_from_slice(&x87.data_selector.to_le_bytes());
    let top = usize::from((x87.status >> 11) & 7);
    for logical in 0..8 {
        output[32 + logical * 16..42 + logical * 16]
            .copy_from_slice(&x87.registers[(top + logical) & 7]);
    }
}

pub(in crate::cpu::x86) fn save_simd(simd: &SimdState, output: &mut [u8]) {
    output[24..28].copy_from_slice(&simd.mxcsr.to_le_bytes());
    output[28..32].copy_from_slice(&0xffffu32.to_le_bytes());
    for index in 0..8 {
        output[160 + index * 16..176 + index * 16].copy_from_slice(&simd.xmm[index]);
    }
}

pub(in crate::cpu::x86) fn restore_x87(input: &[u8]) -> X87State {
    let word = |offset| u16::from_le_bytes(input[offset..offset + 2].try_into().unwrap());
    let dword = |offset| u32::from_le_bytes(input[offset..offset + 4].try_into().unwrap());
    let top = usize::from((word(2) >> 11) & 7);
    let registers: [[u8; 10]; 8] = std::array::from_fn(|physical| {
        let logical = (physical + 8 - top) & 7;
        input[32 + logical * 16..42 + logical * 16]
            .try_into()
            .unwrap()
    });
    let mut tag = 0;
    for (index, register) in registers.iter().enumerate() {
        let significand = u64::from_le_bytes(register[..8].try_into().unwrap());
        let exponent = u16::from_le_bytes(register[8..10].try_into().unwrap()) & 0x7fff;
        let value = if input[4] & (1 << index) == 0 {
            3
        } else if exponent == 0 && significand == 0 {
            1
        } else if exponent == 0 || exponent == 0x7fff || significand >> 63 == 0 {
            2
        } else {
            0
        };
        tag |= value << (index * 2);
    }
    X87State {
        control: word(0),
        status: word(2),
        tag,
        opcode: word(6) & 0x7ff,
        instruction_pointer: dword(8),
        data_pointer: dword(16),
        code_selector: word(12),
        data_selector: word(20),
        registers,
    }
}

pub(in crate::cpu::x86) fn restore_xmm(input: &[u8]) -> [[u8; 16]; 8] {
    std::array::from_fn(|index| {
        input[160 + index * 16..176 + index * 16]
            .try_into()
            .unwrap()
    })
}
