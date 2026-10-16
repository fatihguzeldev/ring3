mod catalog;
mod datafiles;
mod identity;
mod linking;
mod plan;
mod set;

use crate::{loader::LoadError, memory::MemoryError};

pub use catalog::{
    ModuleCatalog32, ModuleLimits32, SystemExport32, SystemExportTarget32, SystemModule32,
};
pub use datafiles::{DataFileLayout32, DataFileModule32, DataFilePlan32};
pub use identity::{ModuleId32, normalize_module_name};
pub use plan::{LoadPlan32, ModuleLoad32, UnloadPlan32};
pub use set::{LoadedModule32, ModuleSet32, ModuleTls32, ResolvedExport32};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleError32 {
    Pe(LoadError),
    Memory(MemoryError),
    InvalidName,
    DuplicateModule,
    DuplicateSymbol,
    MissingModule,
    MissingSymbol,
    InvalidForwarder,
    ForwarderCycle,
    InvalidSystemExport,
    InvalidModuleKind,
    AddressConflict,
    StalePlan,
    ModuleInUse,
    Busy,
    InvalidRequest,
    NotFound,
    InvalidHandle,
    InvalidToken,
    InvalidReturn,
    ReferenceExhausted,
    InitializationFailed,
    Allocation,
    Capacity,
}

impl From<LoadError> for ModuleError32 {
    fn from(value: LoadError) -> Self {
        Self::Pe(value)
    }
}

impl From<MemoryError> for ModuleError32 {
    fn from(value: MemoryError) -> Self {
        Self::Memory(value)
    }
}
