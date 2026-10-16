use crate::windows::nls::NlsError32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateLayout32 {
    LeftToRight,
    RightToLeft,
    Automatic { reading_layout: u32, hebrew: bool },
}

pub(super) struct DateLayoutWriter32 {
    layout: Option<DateLayout32>,
    boundaries: Vec<usize>,
}

impl DateLayoutWriter32 {
    pub(super) fn new(pattern: &str, layout: Option<DateLayout32>) -> Result<Self, NlsError32> {
        let mut boundaries = Vec::new();
        if layout.is_some() {
            boundaries
                .try_reserve(pattern.len())
                .map_err(|_| NlsError32::Allocation)?;
        }
        Ok(Self { layout, boundaries })
    }

    pub(super) fn mark(&mut self, offset: usize) {
        if self.layout.is_some() {
            self.boundaries.push(offset);
        }
    }

    pub(super) fn finish(self, text: String) -> Result<String, NlsError32> {
        let Some(layout) = self.layout else {
            return Ok(text);
        };
        let normalize = matches!(layout, DateLayout32::Automatic { hebrew: true, .. });
        let right_to_left = match layout {
            DateLayout32::LeftToRight => false,
            DateLayout32::RightToLeft => true,
            DateLayout32::Automatic {
                reading_layout,
                hebrew,
            } => {
                if hebrew {
                    text.chars()
                        .any(|value| matches!(value, '\u{0590}'..='\u{05ff}'))
                } else {
                    reading_layout == 1
                }
            }
        };
        let marker: char = if right_to_left {
            '\u{200f}'
        } else {
            '\u{200e}'
        };
        let capacity = self
            .boundaries
            .len()
            .checked_mul(marker.len_utf8())
            .and_then(|extra| text.len().checked_add(extra))
            .ok_or(NlsError32::Allocation)?;
        let mut output = String::new();
        output
            .try_reserve_exact(capacity)
            .map_err(|_| NlsError32::Allocation)?;
        let mut previous = 0;
        for offset in self.boundaries {
            append(&mut output, &text[previous..offset], normalize, marker);
            output.push(marker);
            previous = offset;
        }
        append(&mut output, &text[previous..], normalize, marker);
        Ok(output)
    }
}

fn append(output: &mut String, text: &str, normalize: bool, marker: char) {
    if normalize {
        // hebrew auto layout rewrites existing marks; explicit layout flags preserve them.
        output.extend(text.chars().map(|value| {
            if matches!(value, '\u{200e}' | '\u{200f}') {
                marker
            } else {
                value
            }
        }));
    } else {
        output.push_str(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windows::nls::calendar::{SystemTime32, format_date_with_layout};
    use crate::windows::nls::locales::LocaleData32;

    #[test]
    fn empty_and_punctuation_literals_keep_their_own_direction_boundary() {
        let date = SystemTime32 {
            year: 2001,
            month: 2,
            day: 3,
            ..Default::default()
        };
        let names = LocaleData32::new(0x409, 0x409).unwrap().calendar;
        for (pattern, expected) in [
            ("yyyy''MM", "\u{200e}2001\u{200e}\u{200e}02"),
            (
                "yyyy'---'MM''dd",
                "\u{200e}2001\u{200e}---\u{200e}02\u{200e}\u{200e}03",
            ),
            ("yyyy''''MM", "\u{200e}2001\u{200e}'\u{200e}02"),
        ] {
            assert_eq!(
                format_date_with_layout(date, pattern, &names, Some(DateLayout32::LeftToRight))
                    .unwrap(),
                expected
            );
        }
    }
}
