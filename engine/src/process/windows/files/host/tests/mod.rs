use super::*;
mod environment;
mod mounting;
mod overlay;
fn utf16(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}
fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(16, 0x7777).unwrap();
    engine.configure_files(FileConfig32::default()).unwrap();
    engine
}
fn long_path() -> Vec<u16> {
    let mut path = utf16("\\\\?\\C:\\");
    for index in 0..14 {
        path.extend(utf16(&format!("{index:02}{}\\", "x".repeat(170))));
    }
    path.extend(utf16("asset.bin"));
    path
}
fn append(engine: &mut EngineInstance, status: FileInput32, units: &[u16]) {
    for (offset, chunk) in units.chunks(2048).enumerate() {
        engine
            .append_file_input(status.epoch, status.token, (offset * 2048) as u32, chunk)
            .unwrap();
    }
}
