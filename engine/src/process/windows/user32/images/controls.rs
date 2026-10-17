use super::context::ImageOrigin32;
use crate::{
    loader::ResourceIdentifier32,
    process::{EngineInstance, HostError, windows::gdi::GdiDibSource32},
    windows::{
        gdi::GdiError32,
        user32::{
            User32Error32,
            images::{ImageKind32, LoadImage32, StockImage32},
        },
    },
};

impl EngineInstance {
    pub(in crate::process::windows::user32) fn load_static_control_image(
        &mut self,
        instance: u32,
        name: ResourceIdentifier32,
        kind: ImageKind32,
        style: u32,
    ) -> Result<u32, HostError> {
        self.create_static_control_resource_image(instance, name, kind, style)
            .map_err(|error| match super::calls::failure_code(error) {
                Ok(code) => HostError::User32(User32Error32::Win32(code)),
                Err(error) => error,
            })
    }

    fn create_static_control_resource_image(
        &mut self,
        instance: u32,
        name: ResourceIdentifier32,
        kind: ImageKind32,
        style: u32,
    ) -> Result<u32, HostError> {
        if !matches!(kind, ImageKind32::Bitmap | ImageKind32::Icon) {
            return Err(HostError::User32(User32Error32::Win32(87)));
        }
        self.user32.as_ref().ok_or(HostError::InvalidRequest)?;
        self.prune_user32_images()?;
        let flags = u32::from(kind == ImageKind32::Icon && style & 0x800 == 0) * 0x40;
        let request = LoadImage32::new(kind, 0, 0, flags).map_err(HostError::User32)?;
        let defaults = Self::image_default_extent();
        if instance == 0 {
            let ResourceIdentifier32::Id(id) = name else {
                return Err(HostError::User32(User32Error32::Win32(1814)));
            };
            let id =
                u16::try_from(id).map_err(|_| HostError::User32(User32Error32::Win32(1814)))?;
            let colors = self
                .gdi
                .as_ref()
                .ok_or(HostError::User32(User32Error32::Win32(50)))?
                .system_colors();
            let stock = StockImage32::load(kind, id, &colors).map_err(HostError::User32)?;
            return self.publish_user32_image(
                GdiDibSource32 {
                    info: &stock.info,
                    pixels: None,
                },
                request,
                request.extent(stock.extent, defaults),
                stock.hotspot,
                Some(ImageOrigin32::System { kind, id }),
                None,
            );
        }
        let pixels = self.read_user32_image_resource(
            instance,
            name,
            request,
            defaults,
            self.image_byte_limit()?,
        )?;
        self.publish_user32_image(
            GdiDibSource32 {
                info: &pixels.bytes[pixels.offset..],
                pixels: None,
            },
            request,
            pixels.extent,
            pixels.hotspot,
            Some(ImageOrigin32::Resource(pixels.source)),
            None,
        )
    }

    pub(in crate::process::windows::user32) fn release_static_control_image(
        &mut self,
        handle: u32,
        kind: ImageKind32,
    ) -> Result<(), HostError> {
        let record = self
            .user32
            .as_ref()
            .ok_or(HostError::InvalidRequest)?
            .images
            .record(handle, kind);
        let record = match record {
            Ok(record) => record,
            Err(error) => {
                let gdi = self
                    .gdi
                    .as_ref()
                    .ok_or(HostError::User32(User32Error32::Win32(50)))?;
                if matches!(
                    gdi.bitmap_info(self.memory()?, handle),
                    Err(GdiError32::InvalidHandle)
                ) && matches!(gdi.image_info(handle), Err(GdiError32::InvalidHandle))
                {
                    return Ok(());
                }
                return Err(HostError::User32(error));
            }
        };
        if record.shared {
            return Err(HostError::User32(User32Error32::Win32(87)));
        }
        match self.check_user32_image(handle, kind) {
            Ok(()) => {}
            Err(HostError::Gdi(GdiError32::InvalidHandle)) => {
                self.user32.as_mut().unwrap().images.retire(handle);
                return Ok(());
            }
            Err(error) => return Err(error),
        }
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let runtime = self.gdi.as_mut().unwrap();
        if kind == ImageKind32::Bitmap {
            runtime
                .delete_object(memory, handle)
                .map_err(HostError::Gdi)?;
        } else {
            let receipt = runtime
                .prepare_delete_image(memory, handle)
                .map_err(HostError::Gdi)?;
            runtime
                .commit_delete_image(memory, receipt)
                .map_err(HostError::Gdi)?;
        }
        self.user32.as_mut().unwrap().images.retire(handle);
        Ok(())
    }
}
