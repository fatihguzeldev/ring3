mod callback;
mod datafiles;
mod initialization;
mod input;
mod lifetime;
mod loading;
mod source;
pub(in crate::process) use loading::ModulePlanProbe32;
pub(in crate::process) use source::CapturedModuleRequest32;
pub use source::ModuleSourceWait32;
mod paths;
pub(in crate::process) use paths::CapturedModulePaths32;
mod thread;
mod tls;
pub(in crate::process) use thread::{ModuleRequest32, ModuleThreadState32};
pub(in crate::process) use tls::TlsAllocation;
mod types;

pub use types::{
    ModuleInitializer32, ModuleInitializerKind32, ModuleOperation32, ModuleRuntimeConfig32,
};

use crate::loader::{ModuleCatalog32, ModuleId32, ModuleSet32};
use initialization::ModuleOperation;
use lifetime::ModuleReference;
use tls::InitialThreadTls;

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(in crate::process) struct ModuleProcess32 {
    catalog: ModuleCatalog32,
    input: Option<input::ModuleInput32>,
    loaded: ModuleSet32,
    datafiles: Vec<crate::loader::DataFileModule32>,
    references: Vec<ModuleReference>,
    attached: Vec<ModuleId32>,
    thread_notifications_disabled: Vec<ModuleId32>,
    thread: Option<InitialThreadTls>,
    operations: Vec<ModuleOperation>,
    next_initializer: u64,
    source: Option<source::PendingModuleLoad32>,
    next_source: u64,
    vfs_sources: Vec<source::ModuleVfsSource32>,
    loader_owner: Option<u32>,
    loader_depth: u32,
    waiting_request: Option<thread::ModuleRequest32>,
    windows_runtime: bool,
}
