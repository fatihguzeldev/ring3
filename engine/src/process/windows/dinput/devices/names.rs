use crate::{input::dinput::DirectInputResult32 as H, windows::text::CodePage32};

pub(super) fn ansi_name(units: &[u16], code_page: CodePage32, limit: usize) -> Result<Vec<u8>, H> {
    let boundary = |end: usize| {
        if end != 0
            && end < units.len()
            && (0xd800..=0xdbff).contains(&units[end - 1])
            && (0xdc00..=0xdfff).contains(&units[end])
        {
            end - 1
        } else {
            end
        }
    };
    let encode = |end| {
        code_page
            .encode_windows(&units[..boundary(end)], 0, b'?')
            .map(|(bytes, _)| bytes)
            .map_err(|_| H::OUT_OF_MEMORY)
    };
    let encoded = encode(units.len())?;
    if encoded.len() <= limit {
        return Ok(encoded);
    }
    let (mut low, mut high) = (0, units.len());
    let mut best = Vec::new();
    while low < high {
        let middle = low + (high - low).div_ceil(2);
        let candidate = encode(middle)?;
        if candidate.len() <= limit {
            low = middle;
            best = candidate;
        } else {
            high = middle - 1;
        }
    }
    Ok(best)
}
