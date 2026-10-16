use crate::{
    loader::{DataFileLayout32, ModuleError32},
    memory::{PageRange, Permissions},
    process::EngineInstance,
};
impl EngineInstance {
    pub(in crate::process) fn datafile_reservations(&self) -> impl Iterator<Item = PageRange> + '_ {
        self.modules
            .datafiles
            .iter()
            .map(|module| module.reservation())
    }
    pub(in crate::process) fn resource_mapping_info(
        &self,
        address: u32,
    ) -> Result<Option<(u32, u32, DataFileLayout32)>, ModuleError32> {
        let Some(module) = self.modules.datafiles.iter().find(|module| {
            address >= module.base()
                && u64::from(address) < u64::from(module.base()) + u64::from(module.extent())
        }) else {
            return Ok(None);
        };
        module.check(self.memory.as_ref().ok_or(ModuleError32::InvalidHandle)?)?;
        Ok(Some((module.base(), module.extent(), module.layout())))
    }
    pub(in crate::process) fn validate_resource_mapping_protection(
        &self,
        range: PageRange,
        permissions: Permissions,
    ) -> Result<bool, ModuleError32> {
        let start = range.first as u64 * 4096;
        let end = start + range.count as u64 * 4096;
        let Some(module) = self.modules.datafiles.iter().find(|module| {
            start < u64::from(module.base()) + u64::from(module.extent())
                && u64::from(module.base()) < end
        }) else {
            return Ok(false);
        };
        if start < u64::from(module.base())
            || end > u64::from(module.base()) + u64::from(module.extent())
        {
            return Err(ModuleError32::InvalidRequest);
        }
        module.permits(
            self.memory.as_ref().ok_or(ModuleError32::InvalidHandle)?,
            range,
            permissions,
        )?;
        Ok(true)
    }
    pub(in crate::process) fn commit_resource_mapping_protection(&mut self, range: PageRange) {
        let memory = self
            .memory
            .as_ref()
            .expect("validated resource mapping protection");
        for module in &mut self.modules.datafiles {
            module.refresh_protection(memory, range);
        }
    }
}
