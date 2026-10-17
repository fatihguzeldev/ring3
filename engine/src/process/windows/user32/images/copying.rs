use super::{
    context::{ImageOrigin32, ImageRecord32},
    publication::options,
};
use crate::{
    loader::ResourceIdentifier32 as Id,
    process::{
        EngineInstance, HostError,
        windows::gdi::{GdiDibSource32, GdiImageKind32},
    },
    windows::user32::{
        User32Error32 as E,
        images::{CopyImage32, ImageExtent32, ImageKind32, LoadImage32, StockImage32},
    },
};

impl ImageOrigin32 {
    fn duplicate(&self) -> Result<Self, E> {
        Ok(match self {
            Self::System { kind, id } => Self::System {
                kind: *kind,
                id: *id,
            },
            Self::Resource(source) => {
                let name = match &source.name {
                    Id::Id(id) => Id::Id(*id),
                    Id::Name(name) => {
                        let mut units = Vec::new();
                        units
                            .try_reserve_exact(name.len())
                            .map_err(|_| E::Allocation)?;
                        units.extend_from_slice(name);
                        Id::Name(units)
                    }
                };
                Self::Resource(super::context::ResourceImage32 {
                    module: source.module,
                    group: source.group,
                    kind: source.kind,
                    name,
                    language: source.language,
                })
            }
        })
    }
}

impl EngineInstance {
    pub(super) fn copy_user32_image(&mut self, args: &[u32]) -> Result<u32, HostError> {
        let kind = ImageKind32::try_from(args[1]).map_err(HostError::User32)?;
        let copy = CopyImage32::new(kind, args[2] as i32, args[3] as i32, args[4])
            .map_err(HostError::User32)?;
        let gdi = self.gdi.as_ref().ok_or(HostError::User32(E::Win32(50)))?;
        let (source, monochrome, dib) = if kind == ImageKind32::Bitmap {
            let info = gdi
                .bitmap_info(self.memory()?, args[0])
                .map_err(HostError::Gdi)?;
            (
                ImageExtent32 {
                    width: info.width,
                    height: info.height,
                },
                info.monochrome,
                info.dib_section,
            )
        } else {
            let info = gdi.image_info(args[0]).map_err(HostError::Gdi)?;
            if (info.kind == GdiImageKind32::Icon) != (kind == ImageKind32::Icon) {
                return Err(HostError::User32(E::Win32(6)));
            }
            (
                ImageExtent32 {
                    width: info.width,
                    height: info.height,
                },
                info.monochrome,
                false,
            )
        };
        let extent = copy.extent(source, Self::image_default_extent());
        if copy.return_original()
            && extent == source
            && (copy.flags & 1 == 0 || monochrome)
            && (copy.flags & 0x2000 == 0 || dib)
        {
            return Ok(args[0]);
        }
        let record = self
            .user32
            .as_ref()
            .unwrap()
            .images
            .record(args[0], kind)
            .ok();
        let shared = record.is_some_and(|record| record.shared);
        let replace = (copy.delete_original() && !shared).then_some(args[0]);
        if replace == Some(self.user32.as_ref().unwrap().input.cursor_handle) {
            return Err(HostError::User32(E::Win32(87)));
        }
        let request = LoadImage32::new(
            kind,
            extent.width as i32,
            extent.height as i32,
            copy.flags & 0x2041,
        )
        .map_err(HostError::User32)?;
        if copy.from_resource() {
            let origin = record
                .filter(|record| record.shared)
                .and_then(|record| record.origin.as_ref())
                .ok_or(HostError::User32(E::Win32(87)))?
                .duplicate()
                .map_err(HostError::User32)?;
            return match origin {
                ImageOrigin32::System { kind, id } => {
                    let stock =
                        StockImage32::load(kind, id, &self.gdi.as_ref().unwrap().system_colors())
                            .map_err(HostError::User32)?;
                    self.publish_user32_image(
                        GdiDibSource32 {
                            info: &stock.info,
                            pixels: None,
                        },
                        request,
                        extent,
                        stock.hotspot,
                        None,
                        replace,
                    )
                }
                ImageOrigin32::Resource(source) => {
                    let pixels = self.read_user32_image_module(
                        source,
                        request,
                        Self::image_default_extent(),
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
                        None,
                        replace,
                    )
                }
            };
        }
        let capacity = gdi.config().objects;
        self.user32
            .as_mut()
            .unwrap()
            .images
            .reserve(capacity, replace)
            .map_err(HostError::User32)?;
        let gdi = self.gdi.as_mut().unwrap();
        let memory = self.memory.as_mut().ok_or(HostError::Closed)?;
        let handle = if kind == ImageKind32::Bitmap {
            let prepared = gdi
                .prepare_copy_bitmap(memory, args[0], options(request, extent), replace)
                .map_err(HostError::Gdi)?;
            gdi.check_resource_bitmap(memory, &prepared)
                .map_err(HostError::Gdi)?;
            gdi.commit_resource_bitmap(memory, prepared)
                .map_err(HostError::Gdi)?
        } else {
            let prepared = gdi
                .prepare_copy_image(memory, args[0], options(request, extent), replace)
                .map_err(HostError::Gdi)?;
            gdi.check_image_group(memory, &prepared)
                .map_err(HostError::Gdi)?;
            gdi.commit_image_group(memory, prepared)
                .map_err(HostError::Gdi)?
        };
        let images = &mut self.user32.as_mut().unwrap().images;
        if let Some(old) = replace {
            images.retire(old);
        }
        images.publish(ImageRecord32 {
            handle,
            kind,
            shared: false,
            origin: None,
        });
        Ok(handle)
    }
}
