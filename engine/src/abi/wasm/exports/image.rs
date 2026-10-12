export_process_calls! {
    load_pe32(length: u32);
    load_pe32_at(length: u32, actual_base: u32);
    load_pe32_linked_at(length: u32, actual_base: u32, gate_base: u32);
    load_pe32_linked_v2_at(length: u32, actual_base: u32, gate_base: u32);
    begin_image_input(total: u32);
    append_image_input(offset: u32, length: u32);
    abort_image_input();
    load_pe32_input_at(actual_base: u32);
    load_pe32_linked_v2_input_at(actual_base: u32, gate_base: u32);
    start_loaded_image(stack_base: u32, pages: u32);
    load_pe32_linked_v3_input_at(actual_base: u32, gate_base: u32);
    load_pe32_linked_v4_input_at(actual_base: u32, gate_base: u32);
    load_pe32_linked_v5_input_at(actual_base: u32, gate_base: u32);
    load_pe32_linked_v6_input_at(actual_base: u32, gate_base: u32);
    load_pe32_linked_v7_input_at(actual_base: u32, gate_base: u32);
    load_pe32_linked_v8_input_at(actual_base: u32, gate_base: u32);
}
