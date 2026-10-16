use crate::windows::{
    automation::AutomationError32 as E, nls::calendar::SystemTime32, text::ordinal_key,
};
use unicode_general_category::{GeneralCategory as G, get_general_category};

pub(crate) struct DateSymbols32 {
    pub(crate) order: u32,
    pub(crate) separator: String,
    pub(crate) am: String,
    pub(crate) pm: String,
    pub(crate) months: Vec<(Vec<u16>, u16)>,
    pub(crate) weekdays: Vec<Vec<u16>>,
    pub(crate) current_year: Option<u16>,
    pub(crate) year_max: u16,
}

pub(crate) struct ParsedDate32 {
    pub(crate) time: SystemTime32,
    pub(crate) date: bool,
}

enum Token32 {
    Number { value: u16, digits: usize },
    Word(Vec<u16>),
}

pub(crate) fn parse_date(units: &[u16], symbols: &DateSymbols32) -> Result<ParsedDate32, E> {
    let mut text = String::new();
    text.try_reserve(units.len().checked_mul(3).ok_or(E::Capacity)?)
        .map_err(|_| E::Allocation)?;
    for scalar in char::decode_utf16(units.iter().copied()) {
        text.push(scalar.map_err(|_| E::TypeMismatch)?);
    }
    let mut text = text.trim();
    if text.is_empty() {
        return Err(E::TypeMismatch);
    }
    let mut meridiem = None;
    for (word, afternoon) in [
        (symbols.am.as_str(), false),
        (symbols.pm.as_str(), true),
        ("AM", false),
        ("PM", true),
    ] {
        if word.is_empty() {
            continue;
        }
        let key = ordinal_key(&word.encode_utf16().collect::<Vec<_>>());
        let input = text.encode_utf16().collect::<Vec<_>>();
        if input.len() >= key.len() && ordinal_key(&input[input.len() - key.len()..]) == key {
            let remaining = &input[..input.len() - key.len()];
            let bytes = String::from_utf16(remaining)
                .map_err(|_| E::TypeMismatch)?
                .len();
            text = text[..bytes].trim_end();
            meridiem = Some(afternoon);
            break;
        }
    }
    let mut time = SystemTime32 {
        year: 1899,
        month: 12,
        day: 30,
        ..Default::default()
    };
    let has_time = text.contains(':') || meridiem.is_some();
    if has_time {
        let marker = text.find(':').unwrap_or(text.len());
        let start = text[..marker]
            .char_indices()
            .rfind(|(_, c)| !c.is_ascii_digit())
            .map_or(0, |(at, c)| at + c.len_utf8());
        let clock = &text[start..];
        let mut values = [0u16; 3];
        let mut count = 0;
        for part in clock.split(':') {
            if count == values.len() || part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit())
            {
                return Err(E::TypeMismatch);
            }
            values[count] = part.parse().map_err(|_| E::TypeMismatch)?;
            count += 1;
        }
        if count == 0 || values[1] > 59 || values[2] > 59 {
            return Err(E::TypeMismatch);
        }
        if let Some(afternoon) = meridiem {
            if !(1..=12).contains(&values[0]) {
                return Err(E::TypeMismatch);
            }
            values[0] = values[0] % 12 + if afternoon { 12 } else { 0 };
        } else if values[0] > 23 {
            return Err(E::TypeMismatch);
        }
        (time.hour, time.minute, time.second) = (values[0], values[1], values[2]);
        text = text[..start].trim().trim_end_matches('T').trim_end();
    }
    if text.is_empty() {
        return if has_time {
            Ok(ParsedDate32 { time, date: false })
        } else {
            Err(E::TypeMismatch)
        };
    }
    let mut tokens = tokens(text, symbols)?;
    tokens.retain(|token| !matches!(token, Token32::Word(word) if symbols.weekdays.contains(word)));
    let mut month = None;
    let mut numbers = Vec::new();
    numbers.try_reserve_exact(3).map_err(|_| E::Allocation)?;
    for token in tokens {
        match token {
            Token32::Number { value, digits } => {
                if numbers.len() == 3 {
                    return Err(E::TypeMismatch);
                }
                numbers.push((value, digits));
            }
            Token32::Word(word) => {
                if month.is_some() {
                    return Err(E::TypeMismatch);
                }
                month = symbols
                    .months
                    .iter()
                    .find(|(key, _)| *key == word)
                    .map(|(_, month)| *month);
                if month.is_none() {
                    return Err(E::TypeMismatch);
                }
            }
        }
    }
    let (year, month, day) = if let Some(month) = month {
        match numbers.as_slice() {
            &[(day, _)] => (symbols.current_year.ok_or(E::TypeMismatch)?, month, day),
            &[(first, first_digits), (last, last_digits)] => {
                if first_digits >= 3 || first > 31 {
                    (year(first, first_digits, symbols.year_max), month, last)
                } else {
                    (year(last, last_digits, symbols.year_max), month, first)
                }
            }
            _ => return Err(E::TypeMismatch),
        }
    } else {
        match numbers.as_slice() {
            &[(first, _), (second, _)] => {
                let year = symbols.current_year.ok_or(E::TypeMismatch)?;
                if symbols.order == 1 {
                    (year, second, first)
                } else {
                    (year, first, second)
                }
            }
            &[(first, first_digits), (second, _), (last, last_digits)] => {
                if first_digits >= 3 || symbols.order == 2 {
                    (year(first, first_digits, symbols.year_max), second, last)
                } else if symbols.order == 1 {
                    (year(last, last_digits, symbols.year_max), second, first)
                } else {
                    (year(last, last_digits, symbols.year_max), first, second)
                }
            }
            _ => return Err(E::TypeMismatch),
        }
    };
    time.year = year;
    time.month = month;
    time.day = day;
    Ok(ParsedDate32 { time, date: true })
}

fn year(value: u16, digits: usize, maximum: u16) -> u16 {
    if digits > 2 {
        return value;
    }
    let century = maximum / 100 - u16::from(value > maximum % 100);
    century * 100 + value
}

fn tokens(text: &str, symbols: &DateSymbols32) -> Result<Vec<Token32>, E> {
    let has_word = text.chars().any(char::is_alphabetic);
    if text.contains('.') && !symbols.separator.contains('.') && !has_word {
        return Err(E::TypeMismatch);
    }
    let mut tokens = Vec::new();
    tokens.try_reserve_exact(5).map_err(|_| E::Allocation)?;
    let mut start = None;
    let mut numeric = false;
    for (at, scalar) in text.char_indices().chain([(text.len(), '\0')]) {
        let digit = scalar.is_ascii_digit();
        let letter = scalar.is_alphabetic()
            || matches!(
                get_general_category(scalar),
                G::NonspacingMark | G::SpacingMark | G::EnclosingMark
            );
        if (digit || letter) && (start.is_none() || numeric == digit) {
            if start.is_none() {
                start = Some(at);
                numeric = digit;
            }
            continue;
        }
        if let Some(begin) = start.take() {
            if tokens.len() == 5 {
                return Err(E::TypeMismatch);
            }
            let word = &text[begin..at];
            tokens.push(if numeric {
                Token32::Number {
                    value: word.parse().map_err(|_| E::TypeMismatch)?,
                    digits: word.len(),
                }
            } else {
                Token32::Word(ordinal_key(&word.encode_utf16().collect::<Vec<_>>()))
            });
        }
        if digit || letter {
            start = Some(at);
            numeric = digit;
        } else if scalar != '\0'
            && !scalar.is_whitespace()
            && !matches!(scalar, '/' | '-' | ',' | '.')
            && !symbols.separator.contains(scalar)
        {
            return Err(E::TypeMismatch);
        }
    }
    Ok(tokens)
}
