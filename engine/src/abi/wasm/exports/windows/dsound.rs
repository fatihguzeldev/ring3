export_process_calls! {
    configure_dsound(length:u32);
    register_dsound_module(base:u32);
    dsound_capture_request(index:u32);
    dsound_receive_capture(length:u32);
    dsound_capture_availability(epoch_low:u32,epoch_high:u32,available:u32);
    dsound_observe_playback_loss(epoch_low:u32,epoch_high:u32,lost:u32);
}
