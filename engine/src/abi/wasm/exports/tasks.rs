export_process_calls! {
    configure_task_return_sources();
    configure_task_controls();
    read_task_controls();
    read_task_context();
    task_return();
    cpu_task_return(next_pc: u32);
    cpu_task_return_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32, next_pc: u32);
    task_return_resident(key_low: u32, key_high: u32, id_low: u32, id_high: u32);
}
