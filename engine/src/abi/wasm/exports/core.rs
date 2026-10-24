export_process_calls! {
    core_prepare(pc: u32, instructions: u32, blocks: u32);
    core_lookup(pc: u32);
    core_prepare_executable(pc: u32, instructions: u32, blocks: u32);
    core_lookup_executable(pc: u32);
    core_code_ptr();
    core_code_len();
    core_context_ptr();
    core_recovery_scratch_ptr();
    core_begin();
    core_end();
    core_continuation_format();
    core_continuation_ptr();
    core_chain_resolve_owned(context: u32, pc: u32, retired: u32);
    core_chain_resolve_executable_owned(context: u32, pc: u32, retired: u32, remaining: u32);
    core_numeric_owned(context: u32, slot: u32, generation: u64, pc: u32, ordinal: u32);
    core_precise_owned(context: u32, slot: u32, generation: u64);
    core_end_owned(context: u32, slot: u32, generation: u64, reason: u32);
    core_end_accounted_owned(context: u32, slot: u32, generation: u64, reason: u32, initial_retired: u32, budget: u32);
    core_dispatch_code_ptr();
    core_dispatch_code_len();
    core_link(slot: u32);
    core_unlink(slot: u32, low: u32, high: u32);
    core_chain_next(context: u32);
    core_chain_probe(context: u32, pc: u32);
    core_chain_select(context: u32, pc: u32, retired: u32);
    core_chain_cold(context: u32);
    core_link_owners_ptr();
    core_cache_victim();
    core_cache_status_ptr();
    core_cache_format();
    core_trace_pointer();
    core_trace_status_ptr();
    core_trace_start(root: u32);
    core_trace_finish(root: u32, instructions: u32, blocks: u32);
    core_trace_ready();
    core_trace_root();
    core_numeric(context: u32, ordinal: u32);
    core_precise(context: u32);
    core_import();
    core_export(reason: u32, work: u32);
    core_capture_windows_call();
    core_complete_windows_call(token: u32);
    core_begin_windows_callback(return_pc: u32, return_id: u32);
    core_finish_windows_callback(token: u32);
    core_dispatch_exception();
    core_finish_exception();
    core_write(context: u32, address: u32, width: u32, value: u32);
}

#[cfg(feature = "observability")]
export_process_calls! {
    core_cold_status_ptr();
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_core_read")]
pub extern "C" fn core_read(context: u32, address: u32, width: u32) -> i64 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::core_read(context, address, width)
    }
}

#[allow(unsafe_code)]
#[unsafe(export_name = "ring3_abi_v1_core_cold_run")]
pub extern "C" fn core_cold_run(budget: u32) -> i32 {
    {
        #![forbid(unsafe_code)]
        crate::process::wasm::core_cold_run(budget)
    }
}
