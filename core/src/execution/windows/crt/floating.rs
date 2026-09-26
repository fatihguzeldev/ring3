use super::{Cpu32, DispatchError, Register32};

pub(super) fn floor(cpu: &mut Cpu32, args: &[u32]) -> Result<(), DispatchError> {
    let value = f64::from_bits(u64::from(args[0]) | (u64::from(args[1]) << 32));
    if !value.is_finite() {
        return Err(DispatchError::Unsupported);
    }
    cpu.push_x87_double(value.floor())
        .ok_or(DispatchError::Unsupported)
}

#[derive(Clone, Copy)]
pub(in crate::execution::windows) enum InlineMath {
    Fmod,
    Asin,
    Acos,
    Pow,
}

pub(super) fn inline_math(cpu: &mut Cpu32, math: InlineMath) -> Result<(), DispatchError> {
    let result = match math {
        InlineMath::Fmod => cpu.x87_inline_fmod(),
        InlineMath::Asin => cpu.x87_inline_inverse_trig(false),
        InlineMath::Acos => cpu.x87_inline_inverse_trig(true),
        InlineMath::Pow => cpu.x87_inline_pow(),
    };
    result.ok_or(DispatchError::Unsupported)
}

pub(super) fn to_integer(cpu: &mut Cpu32) -> Result<u32, DispatchError> {
    let bytes = cpu
        .pop_x87_truncated_integer()
        .ok_or(DispatchError::Unsupported)?
        .to_le_bytes();
    cpu.set_register(
        Register32::Edx,
        u32::from_le_bytes(bytes[4..].try_into().expect("high word")),
    );
    Ok(u32::from_le_bytes(bytes[..4].try_into().expect("low word")))
}

const EXCEPTIONS: [(u16, u32); 6] = [
    (0x01, 0x10),
    (0x02, 0x0008_0000),
    (0x04, 0x08),
    (0x08, 0x04),
    (0x10, 0x02),
    (0x20, 0x01),
];

pub(super) fn control(cpu: &mut Cpu32, value: u32, mask: u32) -> u32 {
    let word = cpu.x87_control_word();
    let previous = flags(word);
    // controlfp leaves the denormal exception mask unchanged; this cpu has no sse.
    let mask = mask & 0x0007_031f;
    if mask == 0 {
        return previous;
    }
    let updated = (previous & !mask) | (value & mask);
    let mut word = word & !0x1f3f;
    for (bit, flag) in EXCEPTIONS {
        if updated & flag != 0 {
            word |= bit;
        }
    }
    word |= u16::try_from((updated & 0x300) << 2).expect("rounding fits u16");
    word |= match updated & 0x0003_0000 {
        0 => 0x300,
        0x0001_0000 => 0x200,
        _ => 0,
    };
    if updated & 0x0004_0000 != 0 {
        word |= 0x1000;
    }
    cpu.set_x87_control_word(word);
    flags(word)
}

fn flags(word: u16) -> u32 {
    let mut result = u32::from(word & 0xc00) >> 2;
    for (bit, flag) in EXCEPTIONS {
        if word & bit != 0 {
            result |= flag;
        }
    }
    result |= match word & 0x300 {
        0 => 0x0002_0000,
        0x200 => 0x0001_0000,
        _ => 0,
    };
    if word & 0x1000 != 0 {
        result |= 0x0004_0000;
    }
    result
}
