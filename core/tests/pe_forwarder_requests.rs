use ring3_core::{
    PeForwarderRequest, PeForwarderRequestError as Error, PeForwarderSymbol as Symbol,
    decode_pe_forwarder_request as decode,
};

#[test]
fn named_requests_preserve_opaque_spelling_and_borrowed_ranges() {
    for (module, name) in [
        ("OtherModule", "ring3_target"),
        ("a/b", "c\\d"),
        (":%#", "n:#/%"),
    ] {
        let text = format!("{module}.{name}");
        let before = text.clone();
        let request = decode(&text, text.len() as u64).unwrap();
        assert_eq!(
            request,
            PeForwarderRequest {
                module,
                separator_offset: module.len(),
                symbol: Symbol::Name(name),
            }
        );
        assert!(std::ptr::eq(request.module.as_ptr(), text.as_ptr()));
        let Symbol::Name(borrowed) = request.symbol else {
            panic!("expected name")
        };
        assert!(std::ptr::eq(
            borrowed.as_ptr(),
            text[module.len() + 1..].as_ptr()
        ));
        assert_eq!(decode(&text, u64::MAX).unwrap(), request);
        assert_eq!(text, before);
    }
}

#[test]
fn decimal_ordinals_preserve_spelling_and_the_full_u32_domain() {
    for (digits, value) in [
        ("0", 0),
        ("00000", 0),
        ("00032768", 32768),
        ("65535", 65535),
        ("65536", 65536),
        ("4294967295", u32::MAX),
        ("0004294967295", u32::MAX),
    ] {
        let text = format!("M.#{digits}");
        let request = decode(&text, text.len() as u64).unwrap();
        assert_eq!(request.symbol, Symbol::Ordinal { digits, value });
        let Symbol::Ordinal {
            digits: borrowed, ..
        } = request.symbol
        else {
            panic!("expected ordinal")
        };
        assert!(std::ptr::eq(borrowed.as_ptr(), text[3..].as_ptr()));
        assert_eq!(request.module, "M");
        assert_eq!(request.separator_offset, 1);
    }
}

#[test]
fn structural_refusals_preserve_exact_error_priority() {
    for (text, expected) in [
        ("", Error::Empty),
        ("M", Error::MissingSeparator),
        (".", Error::EmptyModule),
        (".N", Error::EmptyModule),
        ("M.", Error::EmptySymbol),
        ("M.#", Error::MissingOrdinalDigits),
        (
            "..",
            Error::MultipleSeparators {
                first: 0,
                second: 1,
            },
        ),
        (
            "M.dll.N",
            Error::MultipleSeparators {
                first: 1,
                second: 5,
            },
        ),
        (
            "M.#1.2",
            Error::MultipleSeparators {
                first: 1,
                second: 4,
            },
        ),
        (
            "M.#x",
            Error::InvalidOrdinalDigit {
                offset: 3,
                byte: b'x',
            },
        ),
        (
            "M.#+1",
            Error::InvalidOrdinalDigit {
                offset: 3,
                byte: b'+',
            },
        ),
        (
            "M.#-1",
            Error::InvalidOrdinalDigit {
                offset: 3,
                byte: b'-',
            },
        ),
        (
            "M.#1x",
            Error::InvalidOrdinalDigit {
                offset: 4,
                byte: b'x',
            },
        ),
        (
            "M.#4294967296",
            Error::OrdinalOverflow {
                offset: 12,
                total: 429_496_729,
                digit: 6,
            },
        ),
        (
            "M.#4294967296x",
            Error::OrdinalOverflow {
                offset: 12,
                total: 429_496_729,
                digit: 6,
            },
        ),
        (
            "M.#4294967296 ",
            Error::UnsupportedByte {
                offset: 13,
                byte: b' ',
            },
        ),
        (
            "M..é",
            Error::UnsupportedByte {
                offset: 3,
                byte: 0xc3,
            },
        ),
    ] {
        assert_eq!(decode(text, u64::MAX), Err(expected), "{text:?}");
    }
}

#[test]
fn byte_limits_precede_scanning_and_count_utf8_bytes() {
    for text in ["M.N", "M.é", "..", "M.#4294967296"] {
        let length = text.len() as u64;
        assert_eq!(
            decode(text, length - 1),
            Err(Error::LengthLimitExceeded {
                length,
                limit: length - 1
            })
        );
    }
    assert_eq!(decode("", 0), Err(Error::Empty));
    assert_eq!(
        decode("M.é", 4),
        Err(Error::UnsupportedByte {
            offset: 2,
            byte: 0xc3
        })
    );
    assert_eq!(
        decode("M.🙂", 6),
        Err(Error::UnsupportedByte {
            offset: 2,
            byte: 0xf0
        })
    );
    assert!(decode("M.N", 3).is_ok());
    assert!(decode("M.N", u64::MAX).is_ok());
}

#[test]
fn every_ascii_byte_is_checked_in_module_and_name_positions() {
    for byte in 0_u8..=127 {
        for (text, offset) in [
            (format!("{}M.N", char::from(byte)), 0),
            (format!("M.N{}", char::from(byte)), 3),
        ] {
            let result = decode(&text, 4);
            if !(0x21..=0x7e).contains(&byte) {
                assert_eq!(result, Err(Error::UnsupportedByte { offset, byte }));
            } else if byte == b'.' {
                let (first, second) = if offset == 0 { (0, 2) } else { (1, 3) };
                assert_eq!(result, Err(Error::MultipleSeparators { first, second }));
            } else {
                assert!(result.is_ok(), "{text:?}: {result:?}");
            }
        }
    }
}
