export_process_calls! {
    cpu_xsave(address: u32, low: u32, high: u32);
    cpu_xrstor(address: u32, low: u32, high: u32);
    cpu_xsave_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32, address: u32, low: u32, high: u32);
    cpu_xrstor_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32, address: u32, low: u32, high: u32);
}
