export_process_calls! {
 configure_execution(units:u32,wasm_bytes:u32,promotion_hits:u32);
 configure_execution_with_limits(units:u32,wasm_bytes:u32,promotion_hits:u32,unit_instructions:u32,unit_wasm_bytes:u32);
 prepare_cache_entry(low:u32,high:u32,pc:u32);
 pending_cache_request();
 prepare_cache_promotion(low:u32,high:u32);
 acknowledge_cache_eviction(low:u32,high:u32,serial_low:u32,serial_high:u32,victim_low:u32,victim_high:u32,slot:u32);
 install_cached_entry(low:u32,high:u32,serial_low:u32,serial_high:u32,unit_low:u32,unit_high:u32,slot:u32);
 cancel_cache_request(low:u32,high:u32,serial_low:u32,serial_high:u32);
 cache_usage();
}
