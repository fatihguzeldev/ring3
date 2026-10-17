use crate::{
    process::{EngineInstance, HostError},
    windows::{
        controls::ControlImage32,
        gdi::GdiError32,
        user32::{User32Error32, images::ImageKind32},
    },
};

impl EngineInstance {
    pub(in crate::process) fn prepare_static_control_thread_retirement(
        &self,
        images: &[ControlImage32],
        thread: u32,
    ) -> Result<(), HostError> {
        if thread == 0 {
            return Err(HostError::InvalidRequest);
        }
        for image in images {
            let runtime = self.gdi.as_ref().ok_or(HostError::InvalidRequest)?;
            let registry = &self
                .user32
                .as_ref()
                .ok_or(HostError::InvalidRequest)?
                .images;
            let record = match registry.record(image.handle, image.kind) {
                Ok(record) => record,
                Err(error) => {
                    if matches!(
                        runtime.bitmap_info(self.memory()?, image.handle),
                        Err(GdiError32::InvalidHandle)
                    ) && matches!(
                        runtime.image_info(image.handle),
                        Err(GdiError32::InvalidHandle)
                    ) {
                        continue;
                    }
                    return Err(HostError::User32(error));
                }
            };
            if record.shared {
                return Err(HostError::User32(User32Error32::Win32(87)));
            }
            match self.check_user32_image(image.handle, image.kind) {
                Ok(()) => {}
                Err(HostError::Gdi(GdiError32::InvalidHandle)) => continue,
                Err(error) => return Err(error),
            }
            if image.kind == ImageKind32::Bitmap
                && runtime
                    .bitmap_info(self.memory()?, image.handle)
                    .map_err(HostError::Gdi)?
                    .dib_section
            {
                return Err(HostError::User32(User32Error32::Win32(87)));
            }
        }
        Ok(())
    }

    pub(in crate::process) fn commit_static_control_thread_retirement(
        &mut self,
        images: Vec<ControlImage32>,
    ) {
        for image in images {
            self.retire_static_control_image(image).expect(
                "private control image retirement was preflighted before thread publication",
            );
        }
    }

    pub(in crate::process::windows::user32) fn retire_static_control_image(
        &mut self,
        image: ControlImage32,
    ) -> Result<(), HostError> {
        match self.release_static_control_image(image.handle, image.kind) {
            Ok(()) => Ok(()),
            Err(HostError::Gdi(GdiError32::SelectedObject)) => {
                self.user32
                    .as_mut()
                    .unwrap()
                    .images
                    .defer_retirement(image.handle, image.kind);
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    pub(in crate::process) fn collect_retired_static_control_images(
        &mut self,
    ) -> Result<(), HostError> {
        let Some(context) = self.user32.as_ref() else {
            return Ok(());
        };
        let count = context.images.pending_retirement_count();
        if count == 0 {
            return Ok(());
        }
        let mut images = Vec::new();
        images
            .try_reserve_exact(count)
            .map_err(|_| HostError::User32(User32Error32::Allocation))?;
        images.extend(context.images.pending_retirement());
        for image in images {
            self.retire_static_control_image(image)?;
        }
        Ok(())
    }
}
