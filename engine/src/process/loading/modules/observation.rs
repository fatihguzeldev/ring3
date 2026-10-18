use crate::{loader::ModuleId32, process::EngineInstance};
use ring3_observability::{Field, Level, categories::ENGINE_LOADER_MODULES};

impl EngineInstance {
    pub(super) fn observe_mapped_modules(&self, ids: &[ModuleId32]) {
        if !self.observer.enabled(Level::Basic, ENGINE_LOADER_MODULES) {
            return;
        }
        for &id in ids {
            let module = self
                .modules
                .loaded
                .get(id)
                .expect("committed module identity");
            self.observer.instant(
                Level::Basic,
                ENGINE_LOADER_MODULES,
                "module.mapped",
                &[
                    Field::unsigned("module_id", id.get()),
                    Field::string("module", module.identity()),
                    Field::unsigned("base", u64::from(module.base())),
                    Field::unsigned("image_size", u64::from(module.image_size())),
                    Field::boolean("is_system", module.is_system()),
                ],
            );
        }
    }

    pub(super) fn observe_unmapped_modules(&self, ids: &[ModuleId32]) {
        if !self.observer.enabled(Level::Basic, ENGINE_LOADER_MODULES) {
            return;
        }
        for &id in ids {
            self.observer.instant(
                Level::Basic,
                ENGINE_LOADER_MODULES,
                "module.unmapped",
                &[Field::unsigned("module_id", id.get())],
            );
        }
    }
}
