use iced_x86::{Code, Mnemonic as M};

use super::StopReason;

pub(super) fn supports(code: Code) -> bool {
    matches!(
        code,
        Code::Punpcklbw_mm_mmm32
            | Code::Punpcklwd_mm_mmm32
            | Code::Punpckldq_mm_mmm32
            | Code::Packsswb_mm_mmm64
            | Code::Pcmpgtb_mm_mmm64
            | Code::Pcmpgtw_mm_mmm64
            | Code::Pcmpgtd_mm_mmm64
            | Code::Packuswb_mm_mmm64
            | Code::Punpckhbw_mm_mmm64
            | Code::Punpckhwd_mm_mmm64
            | Code::Punpckhdq_mm_mmm64
            | Code::Packssdw_mm_mmm64
            | Code::Pcmpeqb_mm_mmm64
            | Code::Pcmpeqw_mm_mmm64
            | Code::Pcmpeqd_mm_mmm64
            | Code::Psrlw_mm_mmm64
            | Code::Psrld_mm_mmm64
            | Code::Psrlq_mm_mmm64
            | Code::Pmullw_mm_mmm64
            | Code::Psubusb_mm_mmm64
            | Code::Psubusw_mm_mmm64
            | Code::Pand_mm_mmm64
            | Code::Paddusb_mm_mmm64
            | Code::Paddusw_mm_mmm64
            | Code::Pandn_mm_mmm64
            | Code::Psraw_mm_mmm64
            | Code::Psrad_mm_mmm64
            | Code::Pmulhw_mm_mmm64
            | Code::Psubsb_mm_mmm64
            | Code::Psubsw_mm_mmm64
            | Code::Por_mm_mmm64
            | Code::Paddsb_mm_mmm64
            | Code::Paddsw_mm_mmm64
            | Code::Pxor_mm_mmm64
            | Code::Psllw_mm_mmm64
            | Code::Pslld_mm_mmm64
            | Code::Psllq_mm_mmm64
            | Code::Pmaddwd_mm_mmm64
            | Code::Psubb_mm_mmm64
            | Code::Psubw_mm_mmm64
            | Code::Psubd_mm_mmm64
            | Code::Paddb_mm_mmm64
            | Code::Paddw_mm_mmm64
            | Code::Paddd_mm_mmm64
            | Code::Psrlw_mm_imm8
            | Code::Psrld_mm_imm8
            | Code::Psrlq_mm_imm8
            | Code::Psraw_mm_imm8
            | Code::Psrad_mm_imm8
            | Code::Psllw_mm_imm8
            | Code::Pslld_mm_imm8
            | Code::Psllq_mm_imm8
    )
}

pub(super) fn execute(operation: M, left: u64, right: u64) -> Result<u64, StopReason> {
    Ok(match operation {
        M::Pand => left & right,
        M::Pandn => !left & right,
        M::Por => left | right,
        M::Pxor => left ^ right,
        M::Packsswb => pack(left, right, 16, -128, 127),
        M::Packuswb => pack(left, right, 16, 0, 255),
        M::Packssdw => pack(left, right, 32, -32768, 32767),
        M::Punpcklbw => unpack(left, right, 8, 0),
        M::Punpcklwd => unpack(left, right, 16, 0),
        M::Punpckldq => unpack(left, right, 32, 0),
        M::Punpckhbw => unpack(left, right, 8, 32),
        M::Punpckhwd => unpack(left, right, 16, 32),
        M::Punpckhdq => unpack(left, right, 32, 32),
        M::Psllw | M::Psrlw | M::Psraw => shift(operation, left, right, 16),
        M::Pslld | M::Psrld | M::Psrad => shift(operation, left, right, 32),
        M::Psllq | M::Psrlq => shift(operation, left, right, 64),
        M::Pmaddwd => multiply_add(left, right),
        _ => arithmetic(operation, left, right)?,
    })
}

fn signed(value: u64, width: u32) -> i64 {
    (value << (64 - width)).cast_signed() >> (64 - width)
}

fn pack(left: u64, right: u64, width: u32, minimum: i64, maximum: i64) -> u64 {
    let destination_width = width / 2;
    let mask = (1 << destination_width) - 1;
    let mut result = 0;
    for (half, input) in [left, right].into_iter().enumerate() {
        for lane in 0..64 / width {
            let value = signed(input >> (lane * width), width).clamp(minimum, maximum);
            result |= (value.cast_unsigned() & mask)
                << (half * 32 + lane as usize * destination_width as usize);
        }
    }
    result
}

fn unpack(left: u64, right: u64, width: u32, start: u32) -> u64 {
    let mask = (1 << width) - 1;
    let mut result = 0;
    for lane in 0..32 / width {
        let source = start + lane * width;
        let destination = lane * width * 2;
        result |= ((left >> source) & mask) << destination;
        result |= ((right >> source) & mask) << (destination + width);
    }
    result
}

fn shift(operation: M, input: u64, count: u64, width: u32) -> u64 {
    let arithmetic = matches!(operation, M::Psraw | M::Psrad);
    if count >= u64::from(width) && !arithmetic {
        return 0;
    }
    let count = count.min(u64::from(width - 1));
    let mask = u64::MAX >> (64 - width);
    let mut result = 0;
    for lane in 0..64 / width {
        let value = (input >> (lane * width)) & mask;
        let shifted = if arithmetic {
            (signed(value, width) >> count).cast_unsigned()
        } else if matches!(operation, M::Psllw | M::Pslld | M::Psllq) {
            value << count
        } else {
            value >> count
        };
        result |= (shifted & mask) << (lane * width);
    }
    result
}

fn multiply_add(left: u64, right: u64) -> u64 {
    let mut result = 0;
    for pair in 0..2 {
        let offset = pair * 32;
        let product = signed(left >> offset, 16) * signed(right >> offset, 16)
            + signed(left >> (offset + 16), 16) * signed(right >> (offset + 16), 16);
        result |= (product.cast_unsigned() & u64::from(u32::MAX)) << offset;
    }
    result
}

fn arithmetic(operation: M, left: u64, right: u64) -> Result<u64, StopReason> {
    let width = match operation {
        M::Paddb
        | M::Psubb
        | M::Paddsb
        | M::Psubsb
        | M::Paddusb
        | M::Psubusb
        | M::Pcmpeqb
        | M::Pcmpgtb => 8,
        M::Paddw
        | M::Psubw
        | M::Paddsw
        | M::Psubsw
        | M::Paddusw
        | M::Psubusw
        | M::Pcmpeqw
        | M::Pcmpgtw
        | M::Pmullw
        | M::Pmulhw => 16,
        M::Paddd | M::Psubd | M::Pcmpeqd | M::Pcmpgtd => 32,
        _ => return Err(StopReason::UnsupportedInstruction),
    };
    let mask = (1 << width) - 1;
    let minimum = -(1_i64 << (width - 1));
    let maximum = (1_i64 << (width - 1)) - 1;
    let mut result = 0;
    for lane in 0..64 / width {
        let a = (left >> (lane * width)) & mask;
        let b = (right >> (lane * width)) & mask;
        let signed_a = signed(a, width);
        let signed_b = signed(b, width);
        let value = match operation {
            M::Paddb | M::Paddw | M::Paddd => a.wrapping_add(b),
            M::Psubb | M::Psubw | M::Psubd => a.wrapping_sub(b),
            M::Paddsb | M::Paddsw => (signed_a + signed_b)
                .clamp(minimum, maximum)
                .cast_unsigned(),
            M::Psubsb | M::Psubsw => (signed_a - signed_b)
                .clamp(minimum, maximum)
                .cast_unsigned(),
            M::Paddusb | M::Paddusw => (a + b).min(mask),
            M::Psubusb | M::Psubusw => a.saturating_sub(b),
            M::Pcmpeqb | M::Pcmpeqw | M::Pcmpeqd => {
                if a == b {
                    mask
                } else {
                    0
                }
            }
            M::Pcmpgtb | M::Pcmpgtw | M::Pcmpgtd => {
                if signed_a > signed_b {
                    mask
                } else {
                    0
                }
            }
            M::Pmullw => a * b,
            M::Pmulhw => (signed_a * signed_b).cast_unsigned() >> 16,
            _ => return Err(StopReason::UnsupportedInstruction),
        };
        result |= (value & mask) << (lane * width);
    }
    Ok(result)
}
