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
    component_cpu_set_gate_mode(key_low: u32, key_high: u32, mode: u32);
    component_cpu_gate_mode(key_low: u32, key_high: u32);
    component_cpu_set_lock_staging_reuse(key_low: u32, key_high: u32, enabled: u32);
    component_cpu_lock_staging_reuse(key_low: u32, key_high: u32);
    component_cpu_set_graphics_overlap(key_low: u32, key_high: u32, enabled: u32);
    component_cpu_graphics_overlap(key_low: u32, key_high: u32);
    component_cpu_try_complete_gate(key_low: u32, key_high: u32);
    component_cpu_try_inline_gate(key_low: u32, key_high: u32, pc: u32);
    component_cpu_try_direct_gate(key_low: u32, key_high: u32, pc: u32, esp: u32, eax: u32);
    component_cpu_try_direct_gate_mask(key_low: u32, key_high: u32, pc: u32, esp: u32, eax: u32, accepted_mask: u32);
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

#[cfg(feature = "external-cpu")]
#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_component_cpu_collect_dirty")]
pub extern "C" fn component_cpu_collect_dirty(
    key_low: u32,
    key_high: u32,
    epoch_low: u32,
    epoch_high: u32,
    directory: u32,
) -> u32 {
    let Some(tables) = crate::abi::external_memory::ExternalMemory::admit(directory, 4096 * 1025)
    else {
        return 7;
    };
    crate::process::wasm::component_cpu_collect_dirty(
        key_low, key_high, epoch_low, epoch_high, tables,
    )
}
