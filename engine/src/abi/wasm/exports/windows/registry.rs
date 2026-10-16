export_process_calls! {
    begin_registry_seed(total:u32);
    append_registry_seed(el:u32,eh:u32,tl:u32,th:u32,offset:u32,length:u32);
    finish_registry_seed(el:u32,eh:u32,tl:u32,th:u32);
    abort_registry_seed(el:u32,eh:u32,tl:u32,th:u32);
    configure_registry(keys:u32,values:u32,handles:u32,bytes:u32,changes:u32,separate_views:u32);
    register_advapi32_module(base:u32);
    registry_change(index:u32);
    registry_read_change_text(el:u32,eh:u32,sl:u32,sh:u32,rl:u32,rh:u32,part:u32,offset:u32);
    registry_read_change_data(el:u32,eh:u32,sl:u32,sh:u32,rl:u32,rh:u32,offset:u32);
    acknowledge_registry_change(el:u32,eh:u32,sl:u32,sh:u32,rl:u32,rh:u32);
    pending_registry_call();
    resume_registry_call(low:u32,high:u32,token:u32);
}
