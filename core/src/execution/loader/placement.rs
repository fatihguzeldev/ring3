use std::ops::Range;

use super::{Image, LoadError, MemoryError, round_pages};

const POOL: Range<u64> = 0x3000_0000..0x7000_0000;
const ALIGNMENT: u64 = 65536;

pub(super) fn assign(
    program: &Image<'_>,
    images: &mut [Image<'_>],
    reserved: &[Range<u64>],
) -> Result<(), LoadError> {
    let mut claimed = reserved.to_vec();
    let program_range = span(program);
    if let Some(range) = claimed.iter().find(|range| overlaps(range, &program_range)) {
        return Err(MemoryError::AlreadyMapped {
            address: range.start.max(program_range.start),
        }
        .into());
    }
    claimed.push(program_range);
    let mut displaced = Vec::new();
    // preserve every available preferred range before assigning fallback addresses.
    for (index, image) in images.iter().enumerate() {
        let range = span(image);
        if claimed.iter().any(|other| overlaps(other, &range)) {
            displaced.push(index);
        } else {
            claimed.push(range);
        }
    }
    for index in displaced {
        let image = &mut images[index];
        let length = round_pages(u64::from(image.table.headers.optional.size_of_image));
        let mut start = POOL.start;
        loop {
            let range = start..start + length;
            if range.end > POOL.end {
                return Err(LoadError::NoModuleAddress);
            }
            if let Some(other) = claimed.iter().find(|other| overlaps(other, &range)) {
                start = other.end.div_ceil(ALIGNMENT) * ALIGNMENT;
            } else {
                image.base = start;
                claimed.push(range);
                break;
            }
        }
    }
    Ok(())
}

fn span(image: &Image<'_>) -> Range<u64> {
    image.base()..image.base() + round_pages(u64::from(image.table.headers.optional.size_of_image))
}

fn overlaps(left: &Range<u64>, right: &Range<u64>) -> bool {
    left.start < right.end && right.start < left.end
}
