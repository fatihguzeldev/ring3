use super::super::{
    CapturedModulePaths32, ModuleRuntimeConfig32, initialization::SavedModuleContext,
};
use crate::{
    files::{FileReadRequest32, FileSection32},
    loader::{DataFileLayout32, ImportSymbol32},
    process::{HostError, windows::activation::modules::ModuleActivationWork32},
};

pub(in crate::process) enum CapturedModuleRequest32 {
    Load {
        name: String,
        actual_base: Option<u32>,
        config: Option<ModuleRuntimeConfig32>,
        no_resolve: bool,
        paths: CapturedModulePaths32,
    },
    Export {
        handle: u32,
        symbol: ImportSymbol32,
        paths: CapturedModulePaths32,
    },
    DataFile {
        name: String,
        layout: DataFileLayout32,
        flags: u32,
        paths: CapturedModulePaths32,
    },
}
pub(in crate::process) struct PendingModuleLoad32 {
    pub(super) token: u64,
    pub(super) thread: u32,
    pub(super) context: SavedModuleContext,
    pub(super) request: CapturedModuleRequest32,
    pub(super) bytes: Option<FileReadRequest32>,
    pub(super) input: Option<super::acquisition::ModuleFileInput32>,
    pub(super) leases: Vec<super::leases::ModuleSourceLease32>,
    pub(super) works: Option<Vec<ModuleActivationWork32>>,
    pub(super) manifest: Option<String>,
    pub(super) header_identities: Vec<String>,
    pub(super) application_directory: Option<Vec<u16>>,
    pub(super) error: Option<HostError>,
    pub(super) call_token: Option<u32>,
    pub(super) default_graph: Option<std::sync::Arc<crate::windows::activation::ActivationGraph32>>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModuleSourceWait32 {
    pub epoch: u64,
    pub token: u64,
    pub thread: u32,
    pub ready: bool,
    pub request: Option<FileReadRequest32>,
}
impl PendingModuleLoad32 {
    pub(in crate::process) fn ready(&self, engine: &crate::process::EngineInstance) -> bool {
        self.bytes
            .is_none_or(|request| engine.provider_file_request_ready(request))
    }
    pub(in crate::process) fn owns_token(&self, token: u32) -> bool {
        self.call_token == Some(token)
    }
}

pub(super) fn resource_file_section(flags: u32) -> FileSection32 {
    if flags & 0x20 != 0 {
        FileSection32::Image
    } else if flags & 0x40 != 0 {
        FileSection32::ExclusiveData
    } else {
        FileSection32::SharedData
    }
}
impl CapturedModuleRequest32 {
    pub(super) fn file_section(&self) -> FileSection32 {
        match self {
            Self::DataFile { flags, .. } => resource_file_section(*flags),
            _ => FileSection32::Image,
        }
    }
}
