export_process_calls! {
    cpu_abi();
    cpu_begin(low: u32, high: u32);
    cpu_context_ptr();
    cpu_prepare(low: u32, high: u32, pc: u32, maximum: u32);
    cpu_code_ptr();
    cpu_code_len();
    cpu_unit_slot();
    cpu_cache_victim();
    cpu_cache_bytes();
    cpu_dispatch_code_ptr();
    cpu_dispatch_code_len();
    cpu_runtime_read(context: u32, address: u32, width: u32);
    cpu_runtime_write(context: u32, address: u32, width: u32, value: u32);
    cpu_runtime_instruction(context: u32);
    cpu_runtime_instruction_count_low();
    cpu_runtime_instruction_count_high();
    cpu_instruction(low: u32, high: u32);
    cpu_publish(low: u32, high: u32);
    cpu_end(low: u32, high: u32);
    cpu_capture_windows_call();
    cpu_complete_windows_call(token: u32);
    cpu_begin_windows_callback(return_pc: u32, return_id: u32);
    cpu_finish_windows_callback(token: u32);
    cpu_dispatch_exception();
    cpu_finish_exception();
}
