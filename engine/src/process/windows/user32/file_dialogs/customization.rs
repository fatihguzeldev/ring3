use crate::{
    memory::GuestAddress,
    process::{EngineInstance, HostError, windows::commdlg::CapturedFileDialog32},
    windows::{
        commdlg::{ComdlgError32, DialogError32},
        user32::{User32Error32 as E, dialogs::DialogTemplate32},
    },
};
impl EngineInstance {
    pub(super) fn common_dialog_template(
        &mut self,
        request: &CapturedFileDialog32,
    ) -> Result<Option<DialogTemplate32>, HostError> {
        if !request.input.flag(0x40 | 0x80) {
            return Ok(None);
        }
        let context = self.user32.as_ref().ok_or(HostError::InvalidRequest)?;
        let limit = (context.scratch_bounds.count * 4096).min(1 << 20);
        let items = context.state.window_limit().saturating_sub(1);
        if request.input.instance == 0 {
            return Err(HostError::Comdlg(ComdlgError32::Dialog(
                DialogError32::NoInstance,
            )));
        }
        let template = if request.input.flag(0x80) {
            let handle = request.input.instance;
            let borrowed = self.windows_global_memory(handle).ok();
            let (address, bytes) = if let Some(block) = borrowed {
                (block.pointer, (block.bytes as usize).min(limit))
            } else {
                self.windows_runtime_allocation_identity(handle)?;
                (handle, limit)
            };
            let memory = self.memory.as_ref().ok_or(HostError::Closed)?;
            let template = DialogTemplate32::read(
                |offset, bytes| {
                    let pointer = address
                        .checked_add(u32::try_from(offset).map_err(|_| E::Capacity)?)
                        .ok_or(E::Capacity)?;
                    memory.read(GuestAddress(pointer), bytes)?;
                    Ok(())
                },
                bytes,
                items,
            )
            .map_err(HostError::User32)?;
            if let Some(block) = borrowed {
                self.check_windows_global_memory(block)?;
            }
            template
        } else {
            if request.input.template == 0 {
                return Err(HostError::Comdlg(ComdlgError32::Dialog(
                    DialogError32::NoTemplate,
                )));
            }
            let bytes = self
                .user32_resource_bytes(
                    request.input.instance,
                    5,
                    request.input.template,
                    request.wide,
                    limit,
                )
                .map_err(|error| match error {
                    HostError::Resource(crate::process::ResourceError32::Memory(error)) => {
                        HostError::Memory(error)
                    }
                    HostError::Resource(
                        crate::process::ResourceError32::Allocation
                        | crate::process::ResourceError32::Capacity
                        | crate::process::ResourceError32::Busy,
                    ) => error,
                    HostError::Resource(_) => {
                        HostError::Comdlg(ComdlgError32::Dialog(DialogError32::FindResource))
                    }
                    _ => error,
                })?;
            DialogTemplate32::parse(&bytes, items).map_err(HostError::User32)?
        };
        if request.input.flag(0x80000) && template.items.iter().any(|item| item.id == 0x045f) {
            return Err(HostError::User32(E::Win32(50)));
        }
        Ok(Some(template))
    }
}
