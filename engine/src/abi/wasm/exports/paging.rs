export_process_calls! {
    cpu_read_modify8(address: u32);
    cpu_read_modify16(address: u32);
    cpu_read_modify32(address: u32);
    cpu_read_modify(address: u32, length: u32, alignment: u32);
    cpu_fetch_fault(pc: u32, length: u32);
    cpu_fetch_fault_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32, pc: u32, length: u32);
    read_cr2();
}
