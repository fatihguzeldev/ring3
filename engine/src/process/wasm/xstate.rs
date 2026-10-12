use super::{cpu_store, mutate};

pub(crate) fn cpu_xsave_avx(segment: u32, offset: u32, low: u32, high: u32) -> u32 {
    cpu_store(|instance| instance.cpu_xsave_avx(segment, offset, low, high))
}
pub(crate) fn cpu_xrstor_avx(segment: u32, offset: u32, low: u32, high: u32) -> u32 {
    mutate(|instance| instance.cpu_xrstor_avx(segment, offset, low, high))
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn cpu_xsave_avx_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    segment: u32,
    offset: u32,
    low: u32,
    high: u32,
) -> u32 {
    cpu_store(|instance| {
        instance.cpu_xsave_avx_resident(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
            segment,
            offset,
            low,
            high,
        )
    })
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn cpu_xrstor_avx_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    segment: u32,
    offset: u32,
    low: u32,
    high: u32,
) -> u32 {
    mutate(|instance| {
        instance.cpu_xrstor_avx_resident(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
            segment,
            offset,
            low,
            high,
        )
    })
}

pub(crate) fn cpu_xsave(address: u32, low: u32, high: u32) -> u32 {
    cpu_store(|instance| instance.cpu_xsave(address, low, high))
}

pub(crate) fn cpu_xrstor(address: u32, low: u32, high: u32) -> u32 {
    mutate(|instance| instance.cpu_xrstor(address, low, high))
}

pub(crate) fn cpu_xsave_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
    low: u32,
    high: u32,
) -> u32 {
    cpu_store(|instance| {
        instance.cpu_xsave_resident(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
            address,
            low,
            high,
        )
    })
}

pub(crate) fn cpu_xrstor_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    address: u32,
    low: u32,
    high: u32,
) -> u32 {
    mutate(|instance| {
        instance.cpu_xrstor_resident(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
            address,
            low,
            high,
        )
    })
}
