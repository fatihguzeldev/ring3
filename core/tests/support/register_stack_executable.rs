use super::executable;

pub fn pe32() -> Vec<u8> {
    let registers = [0_u8, 1, 2, 3, 5, 6, 7];
    let mut code = Vec::new();
    for (register, value) in registers.into_iter().zip([
        0x1122_3344_u32,
        0x5566_7788,
        0x99aa_bbcc,
        0xddee_ff00,
        0x1020_3040,
        0x5060_7080,
        0x90a0_b0c0,
    ]) {
        code.push(0xb8 + register);
        code.extend_from_slice(&value.to_le_bytes());
    }
    code.push(0x60);
    for register in registers {
        code.push(0xb8 + register);
        code.extend_from_slice(&0xffff_ffff_u32.to_le_bytes());
    }
    code.extend_from_slice(&[0x61, 0x66, 0x60]);
    for (register, value) in registers.into_iter().zip([
        0xa1a1_0000_u32,
        0xa2a2_0000,
        0xa3a3_0000,
        0xa4a4_0000,
        0xa5a5_0000,
        0xa6a6_0000,
        0xa7a7_0000,
    ]) {
        code.push(0xb8 + register);
        code.extend_from_slice(&value.to_le_bytes());
    }
    code.extend_from_slice(&[0x66, 0x61, 0xcc]);
    executable::pe32(&code)
}
