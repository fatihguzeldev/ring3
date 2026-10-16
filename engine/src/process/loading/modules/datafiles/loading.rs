use crate::{
    loader::{DataFileLayout32, ModuleError32},
    process::{EngineInstance, HostError, ModuleOperation32},
};
fn layout(flags: u32) -> Result<DataFileLayout32, ModuleError32> {
    if flags & !0x62 != 0 || flags & 0x62 == 0 || flags & 0x42 == 0x42 {
        return Err(ModuleError32::InvalidRequest);
    }
    Ok(if flags & 0x20 != 0 {
        DataFileLayout32::Image
    } else {
        DataFileLayout32::File
    })
}
impl EngineInstance {
    pub fn load_resource_only_module(
        &mut self,
        name: &str,
        flags: u32,
    ) -> Result<ModuleOperation32, HostError> {
        let layout = layout(flags).map_err(HostError::Module)?;
        self.memory()?;
        self.guard_detection_material_change()?;
        if self.modules.loaded.find(name).is_some() {
            return self.load_library_module(name, None);
        }
        if self.modules.thread.is_none() {
            return Err(HostError::Module(ModuleError32::InvalidRequest));
        }
        self.acquire_loader_thread()?;
        let result = (|| {
            if self.call_cancelled() {
                return Err(HostError::Call(crate::process::CallError::Cancelled));
            }
            let reserved = self.guest_address_reservations()?;
            self.modules
                .datafiles
                .try_reserve(1)
                .map_err(|_| HostError::Module(ModuleError32::Allocation))?;
            let plan = self
                .modules
                .catalog
                .plan_datafile(
                    &self.modules.loaded,
                    self.memory()?,
                    name,
                    layout,
                    &reserved,
                )
                .map_err(HostError::Module)?;
            let handle = plan.handle();
            let module = plan
                .install(
                    self.memory.as_mut().ok_or(HostError::Closed)?,
                    &mut self.modules.loaded,
                )
                .map_err(HostError::Module)?;
            self.modules.datafiles.push(module);
            Ok(ModuleOperation32::Loaded { handle })
        })();
        self.release_loader_thread();
        result
    }
    pub fn free_resource_only_module(
        &mut self,
        handle: u32,
    ) -> Result<ModuleOperation32, HostError> {
        self.memory()?;
        self.guard_detection_material_change()?;
        let index = self
            .modules
            .datafiles
            .iter()
            .position(|module| module.handle() == handle)
            .ok_or(HostError::Module(ModuleError32::InvalidHandle))?;
        self.acquire_loader_thread()?;
        let result = (|| {
            self.modules.datafiles[index]
                .unload(
                    self.memory.as_mut().ok_or(HostError::Closed)?,
                    &mut self.modules.loaded,
                )
                .map_err(HostError::Module)?;
            self.modules.datafiles.remove(index);
            Ok(ModuleOperation32::Released)
        })();
        self.release_loader_thread();
        result
    }
}
