export_process_calls! {
    resume_dinput_file_call(low:u32,high:u32,token:u32);
    cancel_dinput_file_call(low:u32,high:u32,token:u32);
    configure_controller_haptics(epoch_low:u32,epoch_high:u32,slot:u32,axes:u32,max_effects:u32);
    render_dinput_haptics(epoch_low:u32,epoch_high:u32,slot:u32,time_low:u32,time_high:u32);
    pending_dinput_haptics(slot:u32);
    acknowledge_dinput_haptics(epoch_low:u32,epoch_high:u32,slot:u32,sequence_low:u32,sequence_high:u32);
    configure_dinput(length:u32);
    register_dinput_module(base:u32);
    register_dinput8_module(base:u32);
    register_input_controller(length:u32);
    submit_windows_input(length:u32);
    submit_windows_pointer(epoch_low:u32,epoch_high:u32,time_low:u32,time_high:u32,x:u32,y:u32);
    submit_controller_input(epoch_low:u32,epoch_high:u32,length:u32);
    sync_windows_input();
    last_windows_input_delivery_error();
}
