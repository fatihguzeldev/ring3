export_process_calls! {
    configure_process_standard_handles(input:u32,output:u32,error:u32);
    begin_process_launch_input(total:u32);
    append_process_launch_input(offset:u32,length:u32);
    commit_process_launch_input();
    abort_process_launch_input();
}
