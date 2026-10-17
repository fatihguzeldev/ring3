use crate::{
    files::FileContentVersion32,
    process::{calls::call::PendingOwner, windows::files::ProviderFile32},
    windows::{CallFrame32, gdi::GdiApi32},
};
pub(in crate::process::windows::gdi) struct FileFontSource32 {
    pub path: Vec<u16>,
    pub flags: u32,
    pub version: FileContentVersion32,
    pub source: u32,
    pub lease: ProviderFile32,
    pub references: u32,
    pub count: u32,
}
pub(in crate::process::windows::gdi) struct FileFontCall32 {
    pub owner: PendingOwner,
    pub token: u32,
    pub frame: CallFrame32,
    pub api: GdiApi32,
    pub thread: u32,
    pub path: Vec<u16>,
    pub flags: u32,
    pub lease: ProviderFile32,
    pub version: FileContentVersion32,
    pub length: usize,
    pub bytes: Vec<u8>,
}
