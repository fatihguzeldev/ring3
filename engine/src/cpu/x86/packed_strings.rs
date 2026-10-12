use super::ir::EffectiveAddress;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackedStringSource {
    Xmm(u8),
    Memory(EffectiveAddress),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackedStringLength {
    Explicit,
    NullTerminated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackedStringOutput {
    Index,
    Mask,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackedStringCompare {
    pub left: u8,
    pub right: PackedStringSource,
    pub length: PackedStringLength,
    pub output: PackedStringOutput,
    pub control: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackedStringResult {
    pub bits: u16,
    pub flags: u32,
}

fn capacity(control: u8) -> usize {
    if control & 1 == 0 { 16 } else { 8 }
}

fn elements(bytes: [u8; 16], control: u8) -> [i32; 16] {
    let width = 16 / capacity(control);
    let signed = control & 2 != 0;
    let mut out = [0; 16];
    for (index, value) in out[..capacity(control)].iter_mut().enumerate() {
        *value = match (width, signed) {
            (1, false) => i32::from(bytes[index]),
            (1, true) => i32::from(bytes[index] as i8),
            (2, false) => i32::from(u16::from_le_bytes([bytes[index * 2], bytes[index * 2 + 1]])),
            (2, true) => i32::from(i16::from_le_bytes([bytes[index * 2], bytes[index * 2 + 1]])),
            _ => unreachable!(),
        };
    }
    out
}

fn length(mode: PackedStringLength, values: &[i32], explicit: u32) -> usize {
    match mode {
        PackedStringLength::Explicit => {
            (explicit as i32).unsigned_abs().min(values.len() as u32) as usize
        }
        PackedStringLength::NullTerminated => values
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(values.len()),
    }
}

impl PackedStringCompare {
    pub fn valid(self) -> bool {
        self.left < 8
            && match self.right {
                PackedStringSource::Xmm(index) => index < 8,
                PackedStringSource::Memory(_) => true,
            }
    }

    pub fn compare(
        self,
        left: [u8; 16],
        right: [u8; 16],
        eax: u32,
        edx: u32,
        incoming_flags: u32,
    ) -> PackedStringResult {
        let count = capacity(self.control);
        let left = elements(left, self.control);
        let right = elements(right, self.control);
        let left_length = length(self.length, &left[..count], eax);
        let right_length = length(self.length, &right[..count], edx);
        let mut bits = 0u16;
        for index in 0..count {
            let matched = match self.control >> 2 & 3 {
                0 => index < right_length && left[..left_length].contains(&right[index]),
                1 => {
                    index < right_length
                        && left[..left_length]
                            .chunks_exact(2)
                            .any(|range| range[0] <= right[index] && right[index] <= range[1])
                }
                2 => match (index < left_length, index < right_length) {
                    (true, true) => left[index] == right[index],
                    (false, false) => true,
                    _ => false,
                },
                3 => (0..count - index).all(|offset| {
                    offset >= left_length
                        || (index + offset < right_length && left[offset] == right[index + offset])
                }),
                _ => unreachable!(),
            };
            bits |= u16::from(matched) << index;
        }
        let full_mask = (u32::MAX >> (32 - count)) as u16;
        match self.control >> 4 & 3 {
            1 => bits ^= full_mask,
            3 => bits ^= ((1u32 << right_length) - 1) as u16,
            _ => {}
        }
        PackedStringResult {
            bits,
            flags: (incoming_flags & !0x8d5)
                | u32::from(bits != 0)
                | (u32::from(right_length < count) << 6)
                | (u32::from(left_length < count) << 7)
                | (u32::from(bits & 1 != 0) << 11),
        }
    }
}

impl PackedStringResult {
    pub fn index(self, control: u8) -> u32 {
        if self.bits == 0 {
            capacity(control) as u32
        } else if control & 0x40 == 0 {
            self.bits.trailing_zeros()
        } else {
            15 - self.bits.leading_zeros()
        }
    }

    pub fn mask(self, control: u8) -> [u8; 16] {
        let mut out = [0; 16];
        if control & 0x40 == 0 {
            out[..2].copy_from_slice(&self.bits.to_le_bytes());
        } else {
            let width = 16 / capacity(control);
            for index in 0..capacity(control) {
                if self.bits & (1 << index) != 0 {
                    out[index * width..(index + 1) * width].fill(0xff);
                }
            }
        }
        out
    }
}
