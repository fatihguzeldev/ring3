use super::{Dialog32, types::buttons};
use crate::{
    memory::{Access, AddressSpace, GuestAddress},
    process::windows::user32::{User32CallContext32, memory},
    process::{EngineInstance, HostError},
    windows::user32::{User32Api32 as Api, User32Error32 as E},
};
impl EngineInstance {
    pub(in crate::process::windows::user32) fn prepare_user32_dialog(
        &mut self,
        api: Api,
        args: &[u32],
        context: User32CallContext32,
    ) -> Result<Dialog32, HostError> {
        let mut dialog = read(self.memory()?, api, args, context).map_err(HostError::User32)?;
        if matches!(api, Api::MessageBoxIndirectA | Api::MessageBoxIndirectW) {
            let raw = parameters(self.memory()?, args[0]).map_err(HostError::User32)?;
            for (offset, target) in [(12, &mut dialog.text), (16, &mut dialog.caption)] {
                let pointer = u32::from_le_bytes(raw[offset..offset + 4].try_into().unwrap());
                if pointer != 0 && pointer <= u32::from(u16::MAX) {
                    *target =
                        self.resource_string_units(dialog.instance, pointer, dialog.language)?;
                }
            }
        }
        Ok(dialog)
    }
}
pub(super) fn read(
    memory: &AddressSpace,
    api: Api,
    args: &[u32],
    context: User32CallContext32,
) -> Result<Dialog32, E> {
    let indirect = matches!(api, Api::MessageBoxIndirectA | Api::MessageBoxIndirectW);
    let (owner, instance, text, caption, style, icon, help_context, help_callback, language) =
        if indirect {
            let raw = parameters(memory, args[0])?;
            let word = |at| u32::from_le_bytes(raw[at..at + 4].try_into().unwrap());
            if word(0) != 40 || word(36) > u32::from(u16::MAX) {
                return Err(E::Win32(87));
            }
            (
                word(4),
                word(8),
                word(12),
                word(16),
                word(20),
                word(24),
                word(28),
                word(32),
                word(36) as u16,
            )
        } else {
            (
                args[0],
                context.instance,
                args[1],
                args[2],
                args[3],
                0,
                0,
                0,
                0,
            )
        };
    if style & !0x003ff3f7 != 0
        || buttons(style).is_empty()
        || !matches!(style & 0xf0, 0 | 0x10 | 0x20 | 0x30 | 0x40 | 0x80)
        || style & 0x3000 == 0x3000
        || (style & 0x300) >> 8 >= buttons(style).len() as u32
        || style & 0x200000 != 0 && owner != 0
        || style & 0x80 != 0 && (!indirect || icon == 0)
    {
        return Err(E::Win32(87));
    }
    if help_callback != 0 {
        memory
            .check_span(GuestAddress(help_callback), 1, Access::Execute)
            .map_err(|_| E::Win32(998))?;
    }
    let string = |pointer, caption: bool| -> Result<Vec<u16>, E> {
        if pointer == 0 {
            return Ok(if caption {
                "Error".encode_utf16().collect()
            } else {
                Vec::new()
            });
        }
        if indirect && pointer <= u32::from(u16::MAX) {
            return Ok(Vec::new());
        }
        memory::string(memory, pointer, api.wide(), 32767, context.ansi)
    };
    let icon_id = (style & 0x80 != 0 && icon <= u32::from(u16::MAX)).then_some(icon as u16);
    let icon_name = if style & 0x80 != 0 && icon_id.is_none() {
        memory::string(memory, icon, api.wide(), 32767, context.ansi)?
    } else {
        Vec::new()
    };
    Ok(Dialog32 {
        id: 0,
        thread: context.thread_id,
        owner,
        style,
        language,
        help_context,
        help_callback,
        instance,
        caption: string(caption, true)?,
        text: string(text, false)?,
        icon_id,
        icon_name,
        result: None,
        help: false,
    })
}

fn parameters(memory: &AddressSpace, address: u32) -> Result<[u8; 40], E> {
    if address == 0 {
        return Err(E::Win32(87));
    }
    let mut bytes = [0; 40];
    memory.read(GuestAddress(address), &mut bytes)?;
    Ok(bytes)
}
