use iced_x86::CpuidFeature;

// baseline avx state, supported avx2/fma and narrow crypto rules keep their owners.
pub(super) fn required(feature: CpuidFeature) -> Option<(u32, u32, usize, u32)> {
    let (register, bit) = match feature {
        CpuidFeature::SHA512 => (0, 0),
        CpuidFeature::SM3 => (0, 1),
        CpuidFeature::SM4 => (0, 2),
        CpuidFeature::RAO_INT => (0, 3),
        CpuidFeature::AVX_VNNI => (0, 4),
        CpuidFeature::AVX_IFMA => (0, 23),
        CpuidFeature::AVX_VNNI_INT8 => (3, 4),
        CpuidFeature::AVX_NE_CONVERT => (3, 5),
        CpuidFeature::AVX_VNNI_INT16 => (3, 10),
        _ => return None,
    };
    Some((7, 1, register, bit))
}

#[cfg(test)]
#[path = "modern/tests.rs"]
mod tests;
