export_process_calls! {
    register_ntdll_module(base:u32);
    dispatch_resident_windows_exception(kl:u32,kh:u32,ul:u32,uh:u32,el:u32,eh:u32);
    dispatch_windows_exception(kl:u32,kh:u32,el:u32,eh:u32);
    terminate_windows_exception(kl:u32,kh:u32,il:u32,ih:u32);
    pending_windows_exception();
    finish_windows_exception_handler(kl:u32,kh:u32,il:u32,ih:u32);
}
