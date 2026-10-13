use super::{PackedKind as P, ShuffleKind as S};

pub(super) fn unsigned(bytes: &[u8], offset: usize, width: usize) -> u64 {
    let mut value = [0; 8];
    value[..width].copy_from_slice(&bytes[offset..offset + width]);
    u64::from_le_bytes(value)
}

pub(super) fn signed(value: u64, width: usize) -> i64 {
    ((value << (64 - width * 8)) as i64) >> (64 - width * 8)
}

pub(super) fn put(bytes: &mut [u8], offset: usize, width: usize, value: u64) {
    bytes[offset..offset + width].copy_from_slice(&value.to_le_bytes()[..width]);
}

pub(super) fn packed(
    kind: P,
    a: [u8; 16],
    b: [u8; 16],
    size: usize,
    width: usize,
    immediate: Option<u8>,
) -> [u8; 16] {
    let mut out = [0; 16];
    let bits = width * 8;
    let mask = u64::MAX >> (64 - bits);
    let count = immediate
        .map(u64::from)
        .unwrap_or_else(|| unsigned(&b, 0, 8));
    if kind == P::MultipleSumAbsoluteByteDifferences {
        let immediate = immediate.unwrap();
        let source = usize::from(immediate & 3) * 4;
        let destination = usize::from(immediate >> 2 & 1) * 4;
        for lane in 0..8 {
            let sum: u64 = (0..4)
                .map(|byte| u64::from(a[destination + lane + byte].abs_diff(b[source + byte])))
                .sum();
            put(&mut out, lane * 2, 2, sum);
        }
        return out;
    }
    if kind == P::MinimumPositionUnsignedWord {
        let (index, minimum) = (0..8)
            .map(|index| (index, unsigned(&b, index * 2, 2)))
            .min_by_key(|&(index, value)| (value, index))
            .unwrap();
        put(&mut out, 0, 2, minimum);
        put(&mut out, 2, 2, index as u64);
        return out;
    }
    if matches!(
        kind,
        P::HorizontalAdd
            | P::HorizontalSubtract
            | P::HorizontalAddSignedSaturate
            | P::HorizontalSubtractSignedSaturate
    ) {
        let subtract = matches!(
            kind,
            P::HorizontalSubtract | P::HorizontalSubtractSignedSaturate
        );
        let saturate = matches!(
            kind,
            P::HorizontalAddSignedSaturate | P::HorizontalSubtractSignedSaturate
        );
        for lane in 0..size / width {
            let input = if lane < size / width / 2 { &a } else { &b };
            let offset = lane % (size / width / 2) * width * 2;
            let x = signed(unsigned(input, offset, width), width);
            let y = signed(unsigned(input, offset + width, width), width);
            let value = if subtract { x - y } else { x + y };
            put(
                &mut out,
                lane * width,
                width,
                if saturate {
                    value.clamp(i16::MIN as i64, i16::MAX as i64)
                } else {
                    value
                } as u64,
            );
        }
        return out;
    }
    if kind == P::MultiplyAddUnsignedSignedBytes {
        for offset in (0..size).step_by(2) {
            let x = i32::from(a[offset]) * i32::from(b[offset] as i8);
            let y = i32::from(a[offset + 1]) * i32::from(b[offset + 1] as i8);
            put(
                &mut out,
                offset,
                2,
                (x + y).clamp(i16::MIN as i32, i16::MAX as i32) as u64,
            );
        }
        return out;
    }
    if matches!(kind, P::MultiplyUnsignedDword | P::MultiplySignedDword) {
        for offset in (0..size).step_by(8) {
            put(
                &mut out,
                offset,
                8,
                if kind == P::MultiplySignedDword {
                    (signed(unsigned(&a, offset, 4), 4) * signed(unsigned(&b, offset, 4), 4)) as u64
                } else {
                    unsigned(&a, offset, 4) * unsigned(&b, offset, 4)
                },
            );
        }
        return out;
    }
    if kind == P::MultiplyAddWords {
        for offset in (0..size).step_by(4) {
            let x = signed(unsigned(&a, offset, 2), 2) * signed(unsigned(&b, offset, 2), 2);
            let y = signed(unsigned(&a, offset + 2, 2), 2) * signed(unsigned(&b, offset + 2, 2), 2);
            put(&mut out, offset, 4, x.wrapping_add(y) as u64);
        }
        return out;
    }
    if kind == P::SumAbsoluteByteDifferences {
        for offset in (0..size).step_by(8) {
            let sum: u64 = (offset..offset + 8)
                .map(|i| u64::from(a[i].abs_diff(b[i])))
                .sum();
            put(&mut out, offset, 8, sum);
        }
        return out;
    }
    for offset in (0..size).step_by(width) {
        let x = unsigned(&a, offset, width);
        let y = unsigned(&b, offset, width);
        let sx = signed(x, width) as i128;
        let sy = signed(y, width) as i128;
        let min = -(1i128 << (bits - 1));
        let max = (1i128 << (bits - 1)) - 1;
        let result = match kind {
            P::Add => x.wrapping_add(y),
            P::Subtract => x.wrapping_sub(y),
            P::AddSignedSaturate => (sx + sy).clamp(min, max) as u64,
            P::SubtractSignedSaturate => (sx - sy).clamp(min, max) as u64,
            P::AddUnsignedSaturate => x.saturating_add(y).min(mask),
            P::SubtractUnsignedSaturate => x.saturating_sub(y),
            P::And => x & y,
            P::AndNot => !x & y,
            P::Or => x | y,
            P::Xor => x ^ y,
            P::Equal => {
                if x == y {
                    mask
                } else {
                    0
                }
            }
            P::GreaterSigned => {
                if sx > sy {
                    mask
                } else {
                    0
                }
            }
            P::Absolute => sy.unsigned_abs() as u64,
            P::Sign => {
                if sy < 0 {
                    -sx as u64
                } else if sy == 0 {
                    0
                } else {
                    x
                }
            }
            P::MultiplyHighRoundSigned => ((sx * sy + 0x4000) >> 15) as u64,
            P::MultiplyLow => x.wrapping_mul(y),
            P::MultiplyHighSigned => ((sx * sy) >> bits) as u64,
            P::MultiplyHighUnsigned => ((u128::from(x) * u128::from(y)) >> bits) as u64,
            P::AverageUnsigned => (x + y + 1) >> 1,
            P::MinimumSigned => sx.min(sy) as u64,
            P::MaximumSigned => sx.max(sy) as u64,
            P::MinimumUnsigned => x.min(y),
            P::MaximumUnsigned => x.max(y),
            P::ShiftLeft | P::ShiftRight | P::ShiftRightSigned => {
                super::shift::lane_value(kind, x, width, count)
            }
            P::MultipleSumAbsoluteByteDifferences
            | P::MinimumPositionUnsignedWord
            | P::Test
            | P::MultiplyUnsignedDword
            | P::MultiplySignedDword
            | P::MultiplyAddWords
            | P::SumAbsoluteByteDifferences
            | P::HorizontalAdd
            | P::HorizontalSubtract
            | P::HorizontalAddSignedSaturate
            | P::HorizontalSubtractSignedSaturate
            | P::MultiplyAddUnsignedSignedBytes => {
                unreachable!()
            }
        };
        put(&mut out, offset, width, result);
    }
    out
}

pub(super) fn shuffle(
    kind: S,
    a: [u8; 16],
    b: [u8; 16],
    size: usize,
    width: usize,
    immediate: u8,
    mask: [u8; 16],
) -> ([u8; 16], Option<u32>) {
    let mut out = a;
    if let Some((from, sign_extend)) = kind.widening() {
        let from = usize::from(from);
        for lane in 0..size / width {
            let value = unsigned(&b, lane * from, from);
            put(
                &mut out,
                lane * width,
                width,
                if sign_extend {
                    signed(value, from) as u64
                } else {
                    value
                },
            );
        }
        return (out, None);
    }
    match kind {
        S::DuplicateEven | S::DuplicateOdd => {
            for lane in 0..size / width {
                let source = (lane / 2 * 2 + usize::from(kind == S::DuplicateOdd)) * width;
                out[lane * width..(lane + 1) * width].copy_from_slice(&b[source..source + width]);
            }
        }
        S::Bytes => {
            for lane in 0..size {
                out[lane] = if b[lane] & 0x80 != 0 {
                    0
                } else {
                    a[usize::from(b[lane]) & (size - 1)]
                };
            }
        }
        S::AlignRight => {
            out = [0; 16];
            for (lane, byte) in out[..size].iter_mut().enumerate() {
                let index = lane + usize::from(immediate);
                *byte = if index < size {
                    b[index]
                } else if index < size * 2 {
                    a[index - size]
                } else {
                    0
                };
            }
        }
        S::Dwords | S::Words => {
            for i in 0..4 {
                let source = usize::from((immediate >> (i * 2)) & 3) * width;
                out[i * width..(i + 1) * width].copy_from_slice(&b[source..source + width]);
            }
        }
        S::HighWords | S::LowWords => {
            out = b;
            let start = if kind == S::HighWords { 8 } else { 0 };
            for i in 0..4 {
                let source = start + usize::from((immediate >> (i * 2)) & 3) * 2;
                out[start + i * 2..start + i * 2 + 2].copy_from_slice(&b[source..source + 2]);
            }
        }
        S::Singles => {
            for i in 0..4 {
                let input = if i < 2 { &a } else { &b };
                let source = usize::from((immediate >> (i * 2)) & 3) * 4;
                out[i * 4..i * 4 + 4].copy_from_slice(&input[source..source + 4]);
            }
        }
        S::Doubles => {
            let first = usize::from(immediate & 1) * 8;
            let second = usize::from((immediate >> 1) & 1) * 8;
            out[..8].copy_from_slice(&a[first..first + 8]);
            out[8..].copy_from_slice(&b[second..second + 8]);
        }
        S::UnpackLow | S::UnpackHigh => {
            let start = if kind == S::UnpackHigh { size / 2 } else { 0 };
            for i in 0..size / width / 2 {
                let source = start + i * width;
                out[i * width * 2..i * width * 2 + width]
                    .copy_from_slice(&a[source..source + width]);
                out[i * width * 2 + width..(i + 1) * width * 2]
                    .copy_from_slice(&b[source..source + width]);
            }
        }
        S::PackSigned | S::PackUnsigned => {
            for i in 0..size / width {
                let (input, index) = if i < size / width / 2 {
                    (&a, i)
                } else {
                    (&b, i - size / width / 2)
                };
                let value = signed(unsigned(input, index * width * 2, width * 2), width * 2);
                let value = if kind == S::PackUnsigned {
                    value.clamp(0, (1i64 << (width * 8)) - 1)
                } else {
                    value.clamp(-(1i64 << (width * 8 - 1)), (1i64 << (width * 8 - 1)) - 1)
                };
                put(&mut out, i * width, width, value as u64);
            }
        }
        S::ShiftBytesLeft | S::ShiftBytesRight => {
            out = [0; 16];
            let count = usize::from(immediate).min(size);
            if kind == S::ShiftBytesLeft {
                out[count..size].copy_from_slice(&a[..size - count]);
            } else {
                out[..size - count].copy_from_slice(&a[count..size]);
            }
        }
        S::InsertByte | S::InsertWord | S::InsertDword => {
            let offset = usize::from(immediate) % (size / width) * width;
            out[offset..offset + width].copy_from_slice(&b[..width]);
        }
        S::InsertSingle => {
            let source = usize::from(immediate >> 6) * 4;
            let destination = usize::from(immediate >> 4 & 3) * 4;
            out[destination..destination + 4].copy_from_slice(&b[source..source + 4]);
            for lane in 0..4 {
                if immediate & (1 << lane) != 0 {
                    out[lane * 4..lane * 4 + 4].fill(0);
                }
            }
        }
        S::ExtractByte | S::ExtractWord | S::ExtractDword => {
            let offset = usize::from(immediate) % (size / width) * width;
            out[..width].copy_from_slice(&b[offset..offset + width]);
            return (out, Some(unsigned(&b, offset, width) as u32));
        }
        S::BlendImmediate | S::BlendVariable => {
            for lane in 0..size / width {
                let select = if kind == S::BlendImmediate {
                    immediate & (1 << lane) != 0
                } else {
                    mask[(lane + 1) * width - 1] & 0x80 != 0
                };
                if select {
                    out[lane * width..(lane + 1) * width]
                        .copy_from_slice(&b[lane * width..(lane + 1) * width]);
                }
            }
        }
        S::MaskBytes | S::MaskSingles | S::MaskDoubles => {
            let mut mask = 0;
            for i in 0..size / width {
                mask |= u32::from(b[(i + 1) * width - 1] >> 7) << i;
            }
            return (out, Some(mask));
        }
        S::SignExtendBytes
        | S::SignExtendWords
        | S::SignExtendDwords
        | S::ZeroExtendBytes
        | S::ZeroExtendWords
        | S::ZeroExtendDwords => unreachable!(),
        S::MoveLowToHigh => out[8..].copy_from_slice(&b[..8]),
        S::MoveHighToLow => out[..8].copy_from_slice(&b[8..]),
    }
    (out, None)
}
