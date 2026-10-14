use super::mutate;

fn wide(low: u32, high: u32) -> u64 {
    u64::from(low) | (u64::from(high) << 32)
}

pub(crate) fn configure_cpu_clock(
    frequency_low: u32,
    frequency_high: u32,
    epoch_low: u32,
    epoch_high: u32,
) -> u32 {
    mutate(|instance| {
        instance.configure_cpu_clock(
            wide(frequency_low, frequency_high),
            wide(epoch_low, epoch_high),
        )
    })
}

pub(crate) fn observe_cpu_clock(low: u32, high: u32) -> u32 {
    mutate(|instance| instance.observe_cpu_clock(wide(low, high)))
}

pub(crate) fn advance_cpu_clock(low: u32, high: u32) -> u32 {
    mutate(|instance| instance.advance_cpu_clock(wide(low, high)))
}
