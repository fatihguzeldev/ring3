export_process_calls! {
    open(pages: u32, low: u32, high: u32);
    close();
    arena_ptr();
    arena_len();
}

#[cfg(feature = "external-cpu")]
export_process_calls! {
    component_memory_lease(key_low: u32, key_high: u32);
}
