use super::cpu_store;

pub(crate) fn deliver_exception() -> u32 {
    cpu_store(|instance| instance.deliver_exception_request())
}

pub(crate) fn deliver_exception_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
) -> u32 {
    cpu_store(|instance| {
        instance.deliver_exception_resident_request(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
        )
    })
}
