export_process_calls! {
    submit_input(length: u32);
    input_state();
    input_read_queue(max: u32, capacity: u32, consume: u32);
    input_consume(count: u32);
    input_drain_relative();
}
