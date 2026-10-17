use crate::{
    memory::GuestAddress,
    process::windows::callbacks::{
        GuestCallbackScratch32, WindowsCallback32, WindowsCallbackProvider32,
    },
    windows::nls::{
        NlsError32,
        locales::{locale_identity, locale_is_neutral},
    },
};
pub(in crate::process::windows::nls) struct Enumeration32 {
    pub continuation: u64,
    pub thread: u32,
    pub procedure: GuestAddress,
    pub parameter: u32,
    pub legacy: bool,
    pub items: Vec<u32>,
    pub index: usize,
    pub scratch: GuestCallbackScratch32,
}
impl Enumeration32 {
    pub(super) fn bytes(locale: u32, legacy: bool) -> Result<Vec<u8>, NlsError32> {
        if legacy {
            return Ok(format!("{locale:08x}\0").into_bytes());
        }
        Ok(locale_identity(locale)?
            .windows_name()
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect())
    }
    pub(super) fn request(&self) -> WindowsCallback32 {
        let locale =
            locale_identity(self.items[self.index]).expect("validated immutable locale inventory");
        let flags = if locale.alternate_sort() { 4 } else { 1 }
            | if locale_is_neutral(locale) {
                0x10
            } else {
                0x20
            };
        let mut arguments = [0; 16];
        arguments[..3].copy_from_slice(&[self.scratch.address.0, flags, self.parameter]);
        WindowsCallback32 {
            procedure: self.procedure,
            arguments,
            count: if self.legacy { 1 } else { 3 },
            continuation: self.continuation,
            provider: WindowsCallbackProvider32::Nls,
        }
    }
}
