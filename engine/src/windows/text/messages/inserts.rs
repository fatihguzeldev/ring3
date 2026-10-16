use super::{MAX_UNITS, MessageArguments32, MessageError32 as E, append};

#[derive(Default)]
struct Specification32 {
    left: bool,
    plus: bool,
    space: bool,
    alternate: bool,
    zero: bool,
    width: usize,
    precision: Option<usize>,
    stars: usize,
    length: u8,
    conversion: u8,
}
pub(super) struct Insert32 {
    pub units: Vec<u16>,
    pub va_bias: usize,
}
fn number(input: &[u16], position: &mut usize) -> Result<usize, E> {
    let mut value = 0usize;
    while let Some(&unit @ 48..=57) = input.get(*position) {
        value = value
            .checked_mul(10)
            .and_then(|v| v.checked_add(usize::from(unit - 48)))
            .filter(|&v| v <= MAX_UNITS)
            .ok_or(E::MoreData)?;
        *position += 1;
    }
    Ok(value)
}
impl Specification32 {
    fn parse(input: &[u16], index: usize, arguments: &impl MessageArguments32) -> Result<Self, E> {
        let mut value = Self::default();
        if input.is_empty() {
            value.conversion = b's';
            return Ok(value);
        }
        let mut at = 0;
        loop {
            match input.get(at).copied() {
                Some(45) => value.left = true,
                Some(43) => value.plus = true,
                Some(32) => value.space = true,
                Some(35) => value.alternate = true,
                Some(48) => value.zero = true,
                _ => break,
            }
            at += 1;
        }
        if input.get(at) == Some(&42) {
            let width = arguments.word(index)? as i32;
            value.left |= width < 0;
            value.width = width.unsigned_abs() as usize;
            value.stars += 1;
            at += 1;
        } else {
            value.width = number(input, &mut at)?;
        }
        if input.get(at) == Some(&46) {
            at += 1;
            if input.get(at) == Some(&42) {
                let precision = arguments.word(index + value.stars)? as i32;
                value.precision = (precision >= 0).then_some(precision as usize);
                value.stars += 1;
                at += 1;
            } else {
                value.precision = Some(number(input, &mut at)?);
            }
        }
        if value.width > MAX_UNITS || value.precision.is_some_and(|n| n > MAX_UNITS) {
            return Err(E::MoreData);
        }
        if input.get(at) == Some(&73) {
            at += 1;
            if input.get(at..at + 2) == Some(&[54, 52]) {
                value.length = 64;
            } else if input.get(at..at + 2) == Some(&[51, 50]) {
                value.length = 32;
            } else {
                return Err(E::InvalidParameter);
            }
            at += 2;
        } else if let Some(&unit @ (104 | 108 | 119)) = input.get(at) {
            value.length = unit as u8;
            at += 1;
        }
        value.conversion = u8::try_from(*input.get(at).ok_or(E::InvalidParameter)?)
            .map_err(|_| E::InvalidParameter)?;
        if at + 1 != input.len() || !b"sScCdiuoxXp".contains(&value.conversion) {
            return Err(E::InvalidParameter);
        }
        Ok(value)
    }
}

pub(super) fn render(
    input: &[u16],
    index: usize,
    wide: bool,
    array: bool,
    arguments: &impl MessageArguments32,
) -> Result<Insert32, E> {
    let spec = Specification32::parse(input, index, arguments)?;
    if array && spec.length == 64 {
        return Err(E::InvalidParameter);
    }
    let value = arguments.word(index + spec.stars)?;
    let mut units = match spec.conversion {
        b's' | b'S' | b'c' | b'C' => {
            let string_wide = match spec.length {
                b'h' => false,
                b'l' | b'w' => true,
                _ => {
                    if spec.conversion.is_ascii_uppercase() {
                        !wide
                    } else {
                        wide
                    }
                }
            };
            if matches!(spec.conversion, b'c' | b'C') {
                arguments.character(value as u16, string_wide)?
            } else if value == 0 {
                "(null)"
                    .encode_utf16()
                    .take(spec.precision.unwrap_or(6))
                    .collect()
            } else {
                arguments.string(value, string_wide, spec.precision)?
            }
        }
        _ => {
            let bits = if spec.length == 64 {
                u64::from(value) | (u64::from(arguments.word(index + spec.stars + 1)?) << 32)
            } else if spec.length == b'h' {
                u64::from(value as u16)
            } else {
                u64::from(value)
            };
            let signed = matches!(spec.conversion, b'd' | b'i');
            let number = if spec.length == 64 {
                bits as i64
            } else if spec.length == b'h' {
                bits as i16 as i64
            } else {
                bits as i32 as i64
            };
            let magnitude = if signed { number.unsigned_abs() } else { bits };
            let digits = match spec.conversion {
                b'x' => format!("{magnitude:x}"),
                b'X' | b'p' => format!("{magnitude:X}"),
                b'o' => format!("{magnitude:o}"),
                _ => magnitude.to_string(),
            };
            let prefix = if signed && number < 0 {
                "-"
            } else if signed && spec.plus {
                "+"
            } else if signed && spec.space {
                " "
            } else if spec.alternate && magnitude != 0 {
                match spec.conversion {
                    b'x' => "0x",
                    b'X' | b'p' => "0X",
                    b'o' => "0",
                    _ => "",
                }
            } else {
                ""
            };
            let mut result = Vec::new();
            append(&mut result, &prefix.encode_utf16().collect::<Vec<_>>())?;
            let precision = spec
                .precision
                .unwrap_or(if spec.conversion == b'p' { 8 } else { 1 });
            let digits = if spec.precision == Some(0) && magnitude == 0 {
                ""
            } else {
                &digits
            };
            let zeroes = precision.saturating_sub(digits.len()).max(
                if spec.zero && !spec.left && spec.precision.is_none() {
                    spec.width.saturating_sub(prefix.len() + digits.len())
                } else {
                    0
                },
            );
            result
                .try_reserve(zeroes + digits.len())
                .map_err(|_| E::Allocation)?;
            result.resize(result.len() + zeroes, 48);
            result.extend(digits.encode_utf16());
            result
        }
    };
    let padding = spec.width.saturating_sub(units.len());
    if padding != 0 {
        let mut padded = Vec::new();
        padded
            .try_reserve_exact(units.len() + padding)
            .map_err(|_| E::Allocation)?;
        if !spec.left {
            padded.resize(padding, 32);
        }
        padded.extend_from_slice(&units);
        if spec.left {
            padded.resize(padded.len() + padding, 32);
        }
        units = padded;
    }
    Ok(Insert32 {
        units,
        va_bias: usize::from(!array && (spec.stars != 0 || spec.length == 64)),
    })
}
