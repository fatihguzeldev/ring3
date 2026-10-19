export_process_calls! {
    begin_execution_census(key_low: u32, key_high: u32, capacity: u32);
    stop_execution_census(key_low: u32, key_high: u32);
    execution_census_page(key_low: u32, key_high: u32, offset: u32);
}
