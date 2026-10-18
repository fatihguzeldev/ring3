export_process_calls! {
 configure_observability(level:u32,max_bytes:u32,sequence:u32,track_low:u32,track_high:u32,context_length:u32);
 observability_drain();
 observability_finish();
 observability_status();
}
