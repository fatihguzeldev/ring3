use super::{ModuleCatalog32, ModuleError32, ModuleSource32, normalize_module_identity};
use std::sync::Arc;

impl ModuleCatalog32 {
    pub(crate) fn retire_guest_snapshot(&mut self, name: &str) -> Result<(), ModuleError32> {
        let name = normalize_module_identity(name)?;
        let slot = self
            .sources
            .iter()
            .position(|source| source.name() == name)
            .ok_or(ModuleError32::MissingModule)?;
        let ModuleSource32::Guest { bytes, .. } = &*self.sources[slot] else {
            return Err(ModuleError32::InvalidModuleKind);
        };
        let bytes = bytes.len();
        if Arc::strong_count(&self.sources[slot]) == 1 {
            self.source_bytes -= bytes;
            self.sources.remove(slot);
            return Ok(());
        }
        self.collect_retired_sources();
        if self.retired.len() >= self.limits.modules {
            return Err(ModuleError32::Capacity);
        }
        self.retired
            .try_reserve(1)
            .map_err(|_| ModuleError32::Allocation)?;
        self.retired
            .push((Arc::downgrade(&self.sources[slot]), bytes));
        self.sources.remove(slot);
        Ok(())
    }

    pub(super) fn released_source_bytes(&self) -> usize {
        self.retired
            .iter()
            .filter(|(source, _)| source.strong_count() == 0)
            .map(|(_, bytes)| bytes)
            .sum()
    }

    pub(super) fn collect_retired_sources(&mut self) {
        self.retired.retain(|(source, bytes)| {
            if source.strong_count() == 0 {
                self.source_bytes -= bytes;
                false
            } else {
                true
            }
        });
    }
}
