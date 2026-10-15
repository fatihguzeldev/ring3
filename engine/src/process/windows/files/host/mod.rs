mod bytes;
mod environment;
mod input;
mod mounting;
mod overlay;
mod storage;

use super::*;
use crate::{
    files::FileIo32,
    process::{EngineInstance, HostError},
};
use input::FileInputKind32;
pub(super) use input::HostFileInput32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileInput32 {
    pub epoch: u64,
    pub token: u64,
    pub kind: u32,
    pub received: u32,
    pub total: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileMountStart32 {
    pub units: u32,
    pub asset: Option<AssetRef32>,
    pub attributes: u32,
    pub times: FileTimes32,
}

#[cfg(test)]
mod tests;
