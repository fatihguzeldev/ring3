use crate::{
    process::EngineInstance,
    windows::{
        com::{InProcessServer32, InterfaceId},
        ole32::HResult32,
    },
};

impl EngineInstance {
    pub(in crate::process::windows) fn registered_com_proxy_class(
        &self,
        iid: InterfaceId,
    ) -> Result<Option<InterfaceId>, HResult32> {
        self.registry.as_ref().map_or(Ok(None), |runtime| {
            crate::windows::com::registered_proxy(&runtime.native, iid)
        })
    }
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
