export_process_calls! {
    configure_gdi(objects:u32,dcs:u32,saved_states:u32,bitmap_bytes:u32,bitmap_base:u32,bitmap_pages:u32,screen_width:u32,screen_height:u32);
    register_gdi32_module(base:u32);
    pending_gdi_frame(index:u32);
    read_gdi_frame(id_low:u32,id_high:u32,revision_low:u32,revision_high:u32,offset:u32,length:u32);
    acknowledge_gdi_frame(id_low:u32,id_high:u32,revision_low:u32,revision_high:u32);
}
