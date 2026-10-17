use super::context::{ImageOrigin32, ImageRecord32};
use crate::{
    process::{
        EngineInstance, HostError,
        windows::gdi::{
            GdiDibSource32, GdiImageKind32, GdiImageRequest32, ResourceBitmapOptions32,
        },
    },
    windows::user32::{
        Point32, User32Error32 as E,
        images::{ImageExtent32, ImageKind32, LoadImage32},
    },
};

pub(super) fn options(request: LoadImage32, extent: ImageExtent32) -> ResourceBitmapOptions32 {
    ResourceBitmapOptions32 {
        width: extent.width,
        height: extent.height,
        monochrome: request.monochrome(),
        dib_section: request.dib_section(),
        transparent: request.flags & 0x20 != 0,
        map3d: request.flags & 0x1000 != 0,
        vga: request.flags & 0x80 != 0,
    }
}

impl EngineInstance {
    pub(super) fn publish_user32_image(
        &mut self,
        source: GdiDibSource32<'_>,
        request: LoadImage32,
        extent: ImageExtent32,
        hotspot: (u16, u16),
        origin: Option<ImageOrigin32>,
        replace: Option<u32>,
    ) -> Result<u32, HostError> {
        let capacity = self
            .gdi
            .as_ref()
            .ok_or(HostError::User32(E::Win32(50)))?
            .config()
            .objects;
        self.user32
            .as_mut()
            .ok_or(HostError::InvalidRequest)?
            .images
            .reserve(capacity, replace)
            .map_err(HostError::User32)?;
        let options = options(request, extent);
        let runtime = self.gdi.as_mut().unwrap();
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let handle = if request.kind == ImageKind32::Bitmap {
            let prepared = runtime
                .prepare_resource_bitmap(memory, source, options, replace)
                .map_err(HostError::Gdi)?;
            runtime
                .check_resource_bitmap(memory, &prepared)
                .map_err(HostError::Gdi)?;
            runtime
                .commit_resource_bitmap(memory, prepared)
                .map_err(HostError::Gdi)?
        } else {
            let prepared = runtime
                .prepare_image_group(
                    memory,
                    GdiImageRequest32 {
                        kind: if request.kind == ImageKind32::Icon {
                            GdiImageKind32::Icon
                        } else {
                            GdiImageKind32::Cursor
                        },
                        dib: source,
                        hotspot: Point32 {
                            x: i32::from(hotspot.0),
                            y: i32::from(hotspot.1),
                        },
                        width: options.width,
                        height: options.height,
                        monochrome: options.monochrome,
                        transparent: options.transparent,
                        map3d: options.map3d,
                        vga: options.vga,
                    },
                    replace,
                )
                .map_err(HostError::Gdi)?;
            runtime
                .check_image_group(memory, &prepared)
                .map_err(HostError::Gdi)?;
            runtime
                .commit_image_group(memory, prepared)
                .map_err(HostError::Gdi)?
        };
        let images = &mut self.user32.as_mut().unwrap().images;
        if let Some(old) = replace {
            images.retire(old);
        }
        images.publish(ImageRecord32 {
            retiring: false,
            handle,
            kind: request.kind,
            shared: request.shared() && !request.from_file(),
            origin,
        });
        Ok(handle)
    }
}
