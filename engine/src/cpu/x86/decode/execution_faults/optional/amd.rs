use iced_x86::CpuidFeature;

// only these amd subsets have a qualified absent-feature ud contract here.
pub(super) fn required(feature: CpuidFeature) -> Option<(u32, u32, usize, u32)> {
    let (register, bit) = match feature {
        CpuidFeature::SSE4A => (2, 6),
        CpuidFeature::D3NOW => (3, 31),
        CpuidFeature::D3NOWEXT => (3, 30),
        CpuidFeature::XOP => (2, 11),
        CpuidFeature::FMA4 => (2, 16),
        CpuidFeature::LWP => (2, 15),
        // the broadcast and synchronization forms share this extended prerequisite.
        CpuidFeature::INVLPGB => return Some((0x8000_0008, 0, 1, 3)),
        _ => return None,
    };
    Some((0x8000_0001, 0, register, bit))
}

#[cfg(test)]
mod tests;
