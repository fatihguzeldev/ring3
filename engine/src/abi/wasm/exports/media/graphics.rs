export_process_calls! {
    graphics_create_vertex_buffer(length: u32);
    graphics_update_vertex_buffer(epoch_low: u32, epoch_high: u32, slot: u32, generation: u32, first: u32, length: u32);
    graphics_destroy_vertex_buffer(epoch_low: u32, epoch_high: u32, slot: u32, generation: u32);
    graphics_draw_triangles(epoch_low: u32, epoch_high: u32, slot: u32, generation: u32, first: u32, count: u32);
    graphics_clear(r: u32, g: u32, b: u32, a: u32);
    graphics_present(surface: u32);
    graphics_seal_batch();
    graphics_read_batch(epoch_low: u32, epoch_high: u32, seq_low: u32, seq_high: u32, offset: u32, length: u32);
    graphics_acknowledge_batch(epoch_low: u32, epoch_high: u32, seq_low: u32, seq_high: u32);
}
