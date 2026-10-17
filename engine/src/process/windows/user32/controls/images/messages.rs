use super::*;

impl EngineInstance {
    pub(super) fn static_image_message(
        &mut self,
        window: WindowHandle32,
        style_kind: ImageKind32,
        args: &[u32],
        call: User32CallContext32,
    ) -> Result<u32, HostError> {
        let (kind, handle, set) = match args[1] {
            0x170 => (ImageKind32::Icon, args[2], true),
            0x171 => (ImageKind32::Icon, 0, false),
            0x172 | 0x173 => {
                let Ok(kind) = ImageKind32::try_from(args[2]) else {
                    return Ok(0);
                };
                (kind, args[3], args[1] == 0x172)
            }
            _ => return Err(HostError::InvalidRequest),
        };
        if (kind == ImageKind32::Bitmap) != (style_kind == ImageKind32::Bitmap) {
            return Ok(0);
        }
        if !set {
            return Ok(self
                .user32
                .as_ref()
                .unwrap()
                .state
                .control_state(window)
                .map_err(HostError::User32)?
                .image
                .map_or(0, |image| image.handle));
        }
        let image = (handle != 0).then_some(ControlImage32 { kind, handle });
        let extent = image
            .map(|image| self.static_image_extent(image))
            .transpose()?
            .unwrap_or((0, 0));
        let queue = self.prepare_user32_queue_change_for(call.thread_id)?;
        let previous = self
            .user32
            .as_mut()
            .unwrap()
            .bind_static_image(window, image, extent, false)
            .map_err(HostError::User32)?;
        self.commit_user32_queue_change(queue);
        Ok(previous)
    }
}
