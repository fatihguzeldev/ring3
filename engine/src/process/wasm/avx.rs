use super::paging::cpu_paging_store;

pub(crate) fn avx_execute(
    descriptor: u32,
    operands: u32,
    immediate: u32,
    live0: u32,
    live1: u32,
    flags: u32,
) -> u32 {
    cpu_paging_store(|instance| {
        instance.avx_execute(descriptor, operands, immediate, live0, live1, flags)
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn avx_execute_resident(
    key_low: u32,
    key_high: u32,
    id_low: u32,
    id_high: u32,
    descriptor: u32,
    operands: u32,
    immediate: u32,
    live0: u32,
    live1: u32,
    flags: u32,
) -> u32 {
    cpu_paging_store(|instance| {
        instance.avx_execute_resident(
            u64::from(key_low) | (u64::from(key_high) << 32),
            u64::from(id_low) | (u64::from(id_high) << 32),
            descriptor,
            operands,
            immediate,
            live0,
            live1,
            flags,
        )
    })
}
