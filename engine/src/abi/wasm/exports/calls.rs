export_process_calls! {
    capture_call(low: u32, high: u32, generation: u32, convention: u32, words: u32);
    complete_call(low: u32, high: u32, generation: u32, token: u32, result: u32);
    capture_resident_call(key_low: u32, key_high: u32, id_low: u32, id_high: u32, convention: u32, words: u32);
    complete_resident_call(key_low: u32, key_high: u32, id_low: u32, id_high: u32, token: u32, result: u32);
    abandon_call(low: u32, high: u32, token: u32);
    complete_windows_call(low: u32, high: u32, generation: u32, token: u32);
    complete_resident_windows_call(key_low: u32, key_high: u32, id_low: u32, id_high: u32, token: u32);
    select_resident_callback_unit(key_low: u32, key_high: u32, home_low: u32, home_high: u32, callback_token: u32, target_low: u32, target_high: u32);
    capture_active_resident_callback_call(key_low: u32, key_high: u32, id_low: u32, id_high: u32, callback_token: u32, convention: u32, words: u32);
    complete_active_resident_callback_call(key_low: u32, key_high: u32, id_low: u32, id_high: u32, callback_token: u32, inner_token: u32, result: u32);
    complete_active_resident_callback_windows_call(key_low: u32, key_high: u32, id_low: u32, id_high: u32, callback_token: u32, inner_token: u32);
    capture_resident_callback_call(key_low: u32, key_high: u32, id_low: u32, id_high: u32, callback_token: u32, convention: u32, words: u32);
    complete_resident_callback_call(key_low: u32, key_high: u32, id_low: u32, id_high: u32, callback_token: u32, inner_token: u32, result: u32);
    finish_callback(low: u32, high: u32, generation: u32, token: u32);
    finish_resident_callback(key_low: u32, key_high: u32, callback_low: u32, callback_high: u32, token: u32);
    authorize_resident_callback(key_low: u32, key_high: u32, callback_low: u32, callback_high: u32, token: u32);
    abort_callback(low: u32, high: u32, token: u32);
    resume_callback_code(low: u32, high: u32, generation: u32, callback_token: u32, count: u32, gate_count: u32);
    resume_callback_entries(low: u32, high: u32, generation: u32, callback_token: u32, count: u32, gate_count: u32);
    begin_callback(low: u32, high: u32, generation: u32, outer_token: u32, entry_pc: u32, return_pc: u32, return_id: u32, count: u32);
    begin_resident_callback(key_low: u32, key_high: u32, outer_low: u32, outer_high: u32, callback_low: u32, callback_high: u32, outer_token: u32, entry_pc: u32, return_pc: u32, return_id: u32, count: u32);
}
