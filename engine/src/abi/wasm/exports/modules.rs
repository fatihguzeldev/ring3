export_process_calls! {
    configure_module_limits(modules: u32, source_bytes: u32, forwarder_depth: u32, metadata_entries: u32, string_bytes: u32);
    register_kernel32_module(base: u32);
    begin_module_input(total: u32, name_length: u32);
    append_module_input(offset: u32, length: u32);
    commit_module_input();
    abort_module_input();
    start_module_process(name_length: u32, actual_base: u32, stack_base: u32, stack_pages: u32, runtime_base: u32);
    load_library_module(name_length: u32, actual_base: u32);
    resolve_module_export(handle: u32, symbol_kind: u32, value: u32);
    free_library_module(handle: u32);
    detach_modules_for_exit();
    module_initializer();
    finish_module_initializer(token_low: u32, token_high: u32);
    abort_module_operation();
    module_handle(name_length: u32);
    module_gates();
    module_gates_page(start:u32,maximum:u32);
    module_teb();
}
