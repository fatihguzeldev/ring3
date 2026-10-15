mod initialization;
mod input;
mod lifetime;
mod loading;
mod tls;
mod types;

pub use types::{
    ModuleInitializer32, ModuleInitializerKind32, ModuleOperation32, ModuleRuntimeConfig32,
};

use crate::loader::{ModuleCatalog32, ModuleId32, ModuleSet32};
use initialization::ModuleOperation;
use lifetime::ModuleReference;
use tls::InitialThreadTls;

#[derive(Default)]
pub(in crate::process) struct ModuleProcess32 {
    catalog: ModuleCatalog32,
    input: Option<input::ModuleInput32>,
    loaded: ModuleSet32,
    references: Vec<ModuleReference>,
    attached: Vec<ModuleId32>,
    thread: Option<InitialThreadTls>,
    operations: Vec<ModuleOperation>,
    next_initializer: u64,
}
