use crate::{
    memory::GuestAddress,
    process::{
        calls::call::PendingOwner,
        windows::callbacks::{
            GuestCallbackScratch32, WindowsCallback32, WindowsCallbackProvider32,
        },
    },
    windows::{CallFrame32, gdi::EnumeratedFont32, text::CodePage32},
};
pub(in crate::process::windows::gdi) struct FontEnumerationCall32 {
    pub owner: PendingOwner,
    pub token: u32,
    pub frame: CallFrame32,
    pub continuation: u64,
    pub thread: u32,
    pub procedure: GuestAddress,
    pub parameter: u32,
    pub items: Vec<EnumeratedFont32>,
    pub index: usize,
    pub scratch: GuestCallbackScratch32,
    pub wide: bool,
    pub cp: CodePage32,
    pub metrics_offset: u32,
}
impl FontEnumerationCall32 {
    pub(super) fn request(&self) -> WindowsCallback32 {
        let mut arguments = [0; 16];
        arguments[..4].copy_from_slice(&[
            self.scratch.address.0,
            self.scratch.address.0 + self.metrics_offset,
            4,
            self.parameter,
        ]);
        WindowsCallback32 {
            provider: WindowsCallbackProvider32::Gdi,
            procedure: self.procedure,
            arguments,
            count: 4,
            continuation: self.continuation,
        }
    }
}
