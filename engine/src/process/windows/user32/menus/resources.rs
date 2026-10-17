use crate::{
    memory::GuestAddress,
    process::{EngineInstance, HostError},
    windows::user32::{
        User32Api32 as Api, User32Error32 as E,
        accelerators::{Accelerator32, ResourceKey32},
        menus::MenuTemplate32,
    },
};

pub(in crate::process::windows::user32) enum PreparedResource32 {
    Menu(MenuTemplate32),
    Accelerator(Vec<Accelerator32>, ResourceKey32),
}

impl EngineInstance {
    pub(in crate::process::windows::user32) fn prepare_user32_menu_resource(
        &self,
        api: Api,
        args: &[u32],
    ) -> Result<PreparedResource32, HostError> {
        if matches!(api, Api::LoadMenuIndirectA | Api::LoadMenuIndirectW) {
            let memory = self.memory.as_ref().ok_or(HostError::Closed)?;
            return MenuTemplate32::read(
                |offset, output| {
                    let address = args[0]
                        .checked_add(u32::try_from(offset).map_err(|_| E::Win32(87))?)
                        .ok_or(E::Win32(87))?;
                    memory.read(GuestAddress(address), output)?;
                    Ok(())
                },
                1 << 20,
                4096,
                32,
            )
            .map(PreparedResource32::Menu)
            .map_err(HostError::User32);
        }
        let accelerator = matches!(api, Api::LoadAcceleratorsA | Api::LoadAcceleratorsW);
        let data = self.user32_resource_payload(
            args[0],
            if accelerator { 9 } else { 4 },
            args[1],
            api.wide(),
            1 << 20,
        )?;
        if accelerator {
            let entries = Accelerator32::resource(&data.bytes, 4096).map_err(HostError::User32)?;
            return Ok(PreparedResource32::Accelerator(
                entries,
                ResourceKey32 {
                    module: data.module.get(),
                    entry: data.entry,
                },
            ));
        }
        MenuTemplate32::read(
            |offset, output| {
                let end = offset.checked_add(output.len()).ok_or(E::InvalidRequest)?;
                output.copy_from_slice(data.bytes.get(offset..end).ok_or(E::InvalidRequest)?);
                Ok(())
            },
            data.bytes.len(),
            4096,
            32,
        )
        .map(PreparedResource32::Menu)
        .map_err(HostError::User32)
    }
}
