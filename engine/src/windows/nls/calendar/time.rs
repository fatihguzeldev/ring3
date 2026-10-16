pub fn time_pattern(pattern: &str, flags: u32) -> String {
    if flags & 0xf == 0 {
        return pattern.into();
    }
    let mut output = String::new();
    let mut literal = String::new();
    let mut chars = pattern.chars().peekable();
    let mut quoted = false;
    let mut omitted = None;
    while let Some(character) = chars.next() {
        if character == '\'' {
            literal.push(character);
            if quoted && chars.peek() == Some(&'\'') {
                literal.push(chars.next().unwrap());
            } else {
                quoted = !quoted;
            }
            continue;
        }
        if quoted || !matches!(character, 'h' | 'H' | 'm' | 's' | 't') {
            literal.push(character);
            continue;
        }
        let mut count = 1;
        while chars.peek() == Some(&character) {
            chars.next();
            count += 1;
        }
        let omit = matches!(character, 'm' | 's') && flags & 1 != 0
            || character == 's' && flags & 2 != 0
            || character == 't' && flags & 4 != 0;
        if omit {
            literal.clear();
            omitted = Some(character);
            continue;
        }
        if let Some(previous) = omitted {
            if previous != 't' && character == 't' {
                output.extend(literal.chars().filter(|c| c.is_whitespace()));
            }
        } else {
            output.push_str(&literal);
        }
        literal.clear();
        omitted = None;
        let character = if character == 'h' && flags & 8 != 0 {
            'H'
        } else {
            character
        };
        output.push_str(&character.to_string().repeat(count));
    }
    if omitted.is_none() {
        output.push_str(&literal);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows::nls::calendar::{SystemTime32, format_time};
    #[test]
    fn time_flags_remove_only_the_documented_fields_and_delimiters() {
        let time = SystemTime32 {
            hour: 13,
            minute: 56,
            second: 13,
            ..Default::default()
        };
        for (picture, flags, expected) in [
            ("h:mm:ss tt", 1, "1 PM"),
            ("h:mm:ss tt", 2, "1:56 PM"),
            ("h:m:s tt", 8, "13:56:13 PM"),
            ("h1t2tt3m", 4, "156"),
            ("m1s2m3s4", 1, ""),
            ("s1s2s3", 2, ""),
            ("h.@:m.@:s.@:tt", 2, "1.@:56PM"),
            (" HH", 0, " 13"),
            ("''''HH", 0, "'13"),
            ("''HH", 0, "13"),
        ] {
            assert_eq!(
                format_time(time, &time_pattern(picture, flags)).unwrap(),
                expected,
                "{picture}"
            );
        }
    }
}
