mod audio;
mod graphics;
mod input;

export_process_calls! {
    configure_media(sample_rate: u32);
    reset_media();
}
