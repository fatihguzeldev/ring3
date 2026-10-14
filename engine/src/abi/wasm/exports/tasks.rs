export_process_calls! {
    configure_task_return_sources();
    configure_task_controls();
    read_task_controls();
    read_task_context();
    task_return();
    task_return_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32);
}
