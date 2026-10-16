export_process_calls! {
    configure_activation(contexts:u32,frames:u32,threads:u32,manifest_bytes:u32,nodes:u32,assemblies:u32,section_bytes:u32,base:u32,pages:u32);
    pending_activation_call();
    resume_activation_call(low:u32,high:u32,token:u32);
}
