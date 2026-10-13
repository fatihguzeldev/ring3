export_process_calls! {
    compile(count: u32);
    generation();
    compile_with_gates(count: u32, gate_count: u32);
    compile_entries(count: u32, gate_count: u32);
    compile_stopped_entry(key_low: u32, key_high: u32, generation: u32);
    compile_stopped_resident_entry(key_low: u32, key_high: u32);
    compile_resident(count: u32);
    compile_resident_with_gates(count: u32, gate_count: u32);
    compile_resident_entries(count: u32, gate_count: u32);
    find_resident(pc: u32);
    compile_resident_callback_unit(key_low: u32, key_high: u32, home_low: u32, home_high: u32, callback_token: u32, count: u32, gate_count: u32);
    dispatcher_module(low: u32, high: u32);
    resident_module(low: u32, high: u32);
    discard_installed_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32, expected_slot: u32);
    retire_stale_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32);
    discard_unacknowledged_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32);
    acknowledge_resident_installation(key_low: u32, key_high: u32, id_low: u32, id_high: u32, slot: u32);
    find_installed_resident(key_low: u32, key_high: u32, pc: u32);
    guard_dispatch_entry(key_low: u32, key_high: u32, state: u32, exit: u32, cancel: u32);
    guard_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32, state: u32, exit: u32, cancel: u32);
    module_ptr();
    module_len();
    guard(low: u32, high: u32, generation: u32, state: u32, exit: u32, cancel: u32);
    acknowledge_resident_callback_installation(key_low: u32, key_high: u32, home_low: u32, home_high: u32, callback_token: u32, unit_low: u32, unit_high: u32, slot: u32);
}
