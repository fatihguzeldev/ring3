use crate::windows::nls::NlsError32;

pub(super) fn append_number(output: &mut String, mut value: u16) -> Result<(), NlsError32> {
    if !(1..=999).contains(&value) {
        return Err(NlsError32::InvalidParameter);
    }
    let mut letters = ['\0'; 5];
    let mut length = 0;
    while value >= 400 {
        letters[length] = 'ת';
        length += 1;
        value -= 400;
    }
    if value >= 100 {
        letters[length] = ['\0', 'ק', 'ר', 'ש'][usize::from(value / 100)];
        length += 1;
        value %= 100;
    }
    if value == 15 || value == 16 {
        letters[length] = 'ט';
        letters[length + 1] = if value == 15 { 'ו' } else { 'ז' };
        length += 2;
    } else {
        if value >= 10 {
            letters[length] =
                ['\0', 'י', 'כ', 'ל', 'מ', 'נ', 'ס', 'ע', 'פ', 'צ'][usize::from(value / 10)];
            length += 1;
            value %= 10;
        }
        if value != 0 {
            letters[length] =
                ['\0', 'א', 'ב', 'ג', 'ד', 'ה', 'ו', 'ז', 'ח', 'ט'][usize::from(value)];
            length += 1;
        }
    }
    output
        .try_reserve(length * 2 + 1)
        .map_err(|_| NlsError32::Allocation)?;
    for (index, letter) in letters[..length].iter().enumerate() {
        if length > 1 && index == length - 1 {
            output.push('"');
        }
        output.push(*letter);
    }
    if length == 1 {
        output.push('\'');
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_numerals_keep_native_single_and_multiple_letter_punctuation() {
        for (value, expected) in [
            (1, "א'"),
            (10, "י'"),
            (15, "ט\"ו"),
            (16, "ט\"ז"),
            (361, "שס\"א"),
            (460, "ת\"ס"),
            (760, "תש\"ס"),
            (761, "תשס\"א"),
            (784, "תשפ\"ד"),
            (999, "תתקצ\"ט"),
        ] {
            let mut text = String::new();
            append_number(&mut text, value).unwrap();
            assert_eq!(text, expected, "{value}");
        }
        for value in [0, 1000, u16::MAX] {
            let mut text = "held".to_owned();
            assert_eq!(
                append_number(&mut text, value),
                Err(NlsError32::InvalidParameter)
            );
            assert_eq!(text, "held");
        }
    }
}
