mod loading;
mod memory;

use crate::{
    loader::{DataFileModule32, ModuleId32},
    process::EngineInstance,
};
impl EngineInstance {
    pub(in crate::process) fn resource_only_modules(
        &self,
    ) -> impl ExactSizeIterator<Item = &DataFileModule32> {
        self.modules.datafiles.iter()
    }
    pub(in crate::process) fn resource_only_module(
        &self,
        id: ModuleId32,
    ) -> Option<&DataFileModule32> {
        self.modules
            .datafiles
            .iter()
            .find(|module| module.id() == id)
    }
}
