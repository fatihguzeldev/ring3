use crate::{
    process::EngineInstance,
    windows::{
        com::{InProcessServer32, InterfaceId},
        ole32::HResult32,
    },
};

impl EngineInstance {
    pub(in crate::process::windows) fn registered_com_server(
        &self,
        class: InterfaceId,
    ) -> Result<Option<InProcessServer32>, HResult32> {
        let Some(registry) = self.registry.as_ref() else {
            return Ok(None);
        };
        crate::windows::com::registered_server(&registry.native, class, |value| {
            self.expand_windows_environment(value)
        })
    }
}
