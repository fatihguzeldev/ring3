export_process_calls! {
    open(pages: u32, low: u32, high: u32);
    close();
    arena_ptr();
    arena_len();
}

#[cfg(feature = "external-cpu")]
export_process_calls! {
    component_memory_lease(key_low: u32, key_high: u32);
    component_cpu_view(key_low: u32, key_high: u32);
    component_cpu_ack_mappings(key_low: u32, key_high: u32, epoch_low: u32, epoch_high: u32, count: u32);
    component_cpu_dirty(key_low: u32, key_high: u32, count: u32);
    component_cpu_gate(key_low: u32, key_high: u32, pc: u32);
    component_cpu_publish(key_low: u32, key_high: u32, stop: u32, detail: u32, auxiliary: u32, vector: u32, retired: u32, work: u32);
    component_cpu_write_fault(key_low: u32, key_high: u32, address: u32);
    component_cpu_release(key_low: u32, key_high: u32);
}

#[cfg(feature = "external-cpu")]
#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_component_cpu_attach")]
pub extern "C" fn component_cpu_attach(
    key_low: u32,
    key_high: u32,
    address: u32,
    length: u32,
) -> u32 {
    let Some(backing) = crate::abi::external_memory::ExternalMemory::admit(address, length) else {
        return 7;
    };
    crate::process::wasm::component_cpu_attach(key_low, key_high, backing)
}
