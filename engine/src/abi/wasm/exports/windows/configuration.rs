export_process_calls! {
    register_version_module(base:u32);
    pending_version_call();
    resume_version_call(low:u32,high:u32,token:u32);
    configure_resources(base:u32,pages:u32,max_handles:u32,max_callbacks:u32,max_enum_bytes:u32);
    configure_ole32(base:u32,pages:u32,objects:u32,threads:u32);
    register_ole32_module(base:u32);
    register_winmm_module(base:u32);
}
