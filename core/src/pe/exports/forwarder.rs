/// a conservative request borrowing module and symbol spelling from caller text.
/// offsets count bytes in that text, not image or file coordinates.
///
/// ```compile_fail
/// use ring3_core::{PeForwarderRequest, decode_pe_forwarder_request};
/// fn escape() -> PeForwarderRequest<'static> {
///     let text = String::from("M.name");
///     decode_pe_forwarder_request(&text, 64).unwrap()
/// }
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeForwarderRequest<'a> {
    pub module: &'a str,
    pub separator_offset: usize,
    pub symbol: PeForwarderSymbol<'a>,
}

/// ordinal digits retain their original spelling, including leading zeroes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeForwarderSymbol<'a> {
    Name(&'a str),
    Ordinal { digits: &'a str, value: u32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeForwarderRequestError {
    LengthLimitExceeded {
        length: u64,
        limit: u64,
    },
    Empty,
    UnsupportedByte {
        offset: usize,
        byte: u8,
    },
    MissingSeparator,
    MultipleSeparators {
        first: usize,
        second: usize,
    },
    EmptyModule,
    EmptySymbol,
    MissingOrdinalDigits,
    InvalidOrdinalDigit {
        offset: usize,
        byte: u8,
    },
    OrdinalOverflow {
        offset: usize,
        total: u32,
        digit: u8,
    },
}

/// decodes a visible-ascii request with exactly one dot and nonempty parts.
/// a symbol beginning with # must contain decimal digits fitting a u32.
/// this is a conservative subset; refusal does not imply windows rejects the text.
///
/// module and name spelling remain opaque: no path normalization, case folding,
/// extension inference, module lookup or forwarder traversal occurs. raw export
/// readers remain independent. the caller byte cap is checked before scanning;
/// this allocation-free operation does not bound acquisition or process memory.
///
/// # errors
/// checks byte length, empty input, the first non-visible-ascii byte, separator
/// count, empty module, empty symbol, and ordinal digits in that order. numeric
/// overflow precedes any later invalid digit. offsets are caller-text byte offsets.
///
/// ```
/// use ring3_core::{PeForwarderSymbol, decode_pe_forwarder_request};
/// let request = decode_pe_forwarder_request("OtherModule.#00032768", 64).unwrap();
/// assert_eq!(request.module, "OtherModule");
/// assert_eq!(request.symbol, PeForwarderSymbol::Ordinal { digits: "00032768", value: 32768 });
/// ```
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn decode_pe_forwarder_request(
    text: &str,
    max_bytes: u64,
) -> Result<PeForwarderRequest<'_>, PeForwarderRequestError> {
    let bytes = text.as_bytes();
    let length = bytes.len() as u64;
    if length > max_bytes {
        return Err(PeForwarderRequestError::LengthLimitExceeded {
            length,
            limit: max_bytes,
        });
    }
    if bytes.is_empty() {
        return Err(PeForwarderRequestError::Empty);
    }
    for (offset, &byte) in bytes.iter().enumerate() {
        if !(0x21..=0x7e).contains(&byte) {
            return Err(PeForwarderRequestError::UnsupportedByte { offset, byte });
        }
    }
    let mut dots = bytes
        .iter()
        .enumerate()
        .filter_map(|(offset, &byte)| (byte == b'.').then_some(offset));
    let separator_offset = dots
        .next()
        .ok_or(PeForwarderRequestError::MissingSeparator)?;
    if let Some(second) = dots.next() {
        return Err(PeForwarderRequestError::MultipleSeparators {
            first: separator_offset,
            second,
        });
    }
    if separator_offset == 0 {
        return Err(PeForwarderRequestError::EmptyModule);
    }
    let module = &text[..separator_offset];
    let symbol = &text[separator_offset + 1..];
    if symbol.is_empty() {
        return Err(PeForwarderRequestError::EmptySymbol);
    }
    let symbol = if let Some(digits) = symbol.strip_prefix('#') {
        if digits.is_empty() {
            return Err(PeForwarderRequestError::MissingOrdinalDigits);
        }
        let mut total = 0_u32;
        for (index, byte) in digits.bytes().enumerate() {
            let offset = separator_offset + 2 + index;
            if !byte.is_ascii_digit() {
                return Err(PeForwarderRequestError::InvalidOrdinalDigit { offset, byte });
            }
            let digit = byte - b'0';
            total = total
                .checked_mul(10)
                .and_then(|n| n.checked_add(u32::from(digit)))
                .ok_or(PeForwarderRequestError::OrdinalOverflow {
                    offset,
                    total,
                    digit,
                })?;
        }
        PeForwarderSymbol::Ordinal {
            digits,
            value: total,
        }
    } else {
        PeForwarderSymbol::Name(symbol)
    };
    Ok(PeForwarderRequest {
        module,
        separator_offset,
        symbol,
    })
}
