mod lifecycle;
mod messages;

use crate::{
    process::{
        EngineInstance, HostError,
        windows::user32::{User32CallContext32, User32Outcome32},
    },
    windows::{
        controls::{ControlImage32, ControlKind32, static_image_kind},
        user32::{User32Error32 as E, images::ImageKind32},
        window::WindowHandle32,
    },
};

impl EngineInstance {
    pub(in crate::process::windows::user32) fn execute_static_image_message(
        &mut self,
        args: &[u32],
        wide: bool,
        call: User32CallContext32,
    ) -> Result<Option<User32Outcome32>, HostError> {
        if !matches!(args[1], 1 | 2 | 0x82 | 0x0c..=0x0e | 0x30 | 0x31 | 0x170..=0x173) {
            return Ok(None);
        }
        let context = self.user32.as_ref().ok_or(HostError::InvalidRequest)?;
        let window = context
            .owned(args[0], call.thread_id)
            .map_err(HostError::User32)?;
        let data = context.state.record(window).map_err(HostError::User32)?;
        if context.state.standard_kind(data.class_atom) != Some(ControlKind32::Static) {
            return Ok(None);
        }
        let Some(kind) = static_image_kind(data.style) else {
            return Ok(matches!(args[1], 0x170..=0x173).then_some(User32Outcome32::Return(0)));
        };
        let result = match args[1] {
            1 => self.create_static_image(window, args[3], wide, call)?,
            2 | 0x82 => {
                self.release_static_image_window(window)?;
                0
            }
            0x170..=0x173 => self.static_image_message(window, kind, args, call)?,
            // image controls do not expose resource names as editable window text or use a font.
            _ => 0,
        };
        Ok(Some(User32Outcome32::Return(result)))
    }

    fn static_image_extent(&self, image: ControlImage32) -> Result<(u32, u32), HostError> {
        let gdi = self.gdi.as_ref().ok_or(HostError::User32(E::Win32(50)))?;
        if image.kind == ImageKind32::Bitmap {
            let info = gdi
                .bitmap_info(self.memory()?, image.handle)
                .map_err(HostError::Gdi)?;
            Ok((info.width, info.height))
        } else {
            let info = gdi.image_info(image.handle).map_err(HostError::Gdi)?;
            if (info.kind == crate::windows::gdi::GdiImageKind32::Icon)
                != (image.kind == ImageKind32::Icon)
            {
                return Err(HostError::User32(E::Win32(6)));
            }
            Ok((info.width, info.height))
        }
    }
}

#[cfg(test)]
mod tests;
