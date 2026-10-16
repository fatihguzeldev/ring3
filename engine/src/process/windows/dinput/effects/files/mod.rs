mod continuation;
mod guest;
mod marshaling;
mod requests;

use crate::{
    files::FileReadRequest32,
    input::dinput::MethodGate,
    process::{calls::call::PendingOwner, windows::files::ProviderFile32},
    windows::CallFrame32,
};
pub(in crate::process::windows::dinput) const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;
pub(in crate::process::windows::dinput) const MAX_ENTRIES: usize = 256;
pub(in crate::process::windows::dinput) struct PendingEffectFile32 {
    pub owner: PendingOwner,
    pub token: u32,
    pub gate: MethodGate,
    pub frame: CallFrame32,
    pub lease: Option<ProviderFile32>,
    pub request: Option<FileReadRequest32>,
    pub stage: FileStage32,
}
pub(in crate::process::windows::dinput) enum FileStage32 {
    Read(usize),
    Ready(Vec<u8>),
    Failed(u32),
    Complete(crate::process::WindowsProviderOutcome32),
}
