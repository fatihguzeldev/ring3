export_process_calls! {
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
