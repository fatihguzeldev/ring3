export_process_calls! {
    avx_execute(descriptor: u32, operands: u32, immediate: u32, live0: u32, live1: u32, flags: u32);
    avx_execute_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32, descriptor: u32, operands: u32, immediate: u32, live0: u32, live1: u32, flags: u32);
}
