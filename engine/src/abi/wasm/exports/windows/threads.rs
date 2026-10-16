export_process_calls! {
    configure_threads(threads:u32,handles:u32,apcs:u32,stack_pages:u32,stack_reserve_pages:u32);
    schedule_windows_thread();
    pending_windows_thread_schedule();
    report_windows_thread_execution(el:u32,eh:u32,dl:u32,dh:u32);
}
