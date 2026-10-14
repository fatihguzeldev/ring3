export_process_calls! {
    configure_cpu_clock(frequency_low: u32, frequency_high: u32, epoch_low: u32, epoch_high: u32);
    observe_cpu_clock(low: u32, high: u32);
    advance_cpu_clock(low: u32, high: u32);
}
