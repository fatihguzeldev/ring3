use super::*;
mod io;
mod lifetime;
mod namespace;
mod termination;
fn utf16(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}
fn vfs() -> Vfs32 {
    Vfs32::new(1, FileLimits32::default()).unwrap()
}
fn open(vfs: &mut Vfs32, path: &str, options: FileOpenOptions32) -> FileOpenResult32 {
    let receipt = vfs.prepare_open(&utf16(path), options).unwrap();
    vfs.commit_open(receipt)
}
