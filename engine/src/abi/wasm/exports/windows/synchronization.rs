export_process_calls! {
    configure_synchronization(events:u32,handles:u32,bindings:u32,waits:u32);
    pending_windows_wait();
    resume_windows_wait(low:u32,high:u32,token:u32);
    cancel_windows_wait(low:u32,high:u32,token:u32);
    signal_windows_event(epoch_low:u32,epoch_high:u32,handle:u32);
}
