use super::context::ResourceImage32;
use crate::{
    loader::ResourceIdentifier32 as Id,
    process::{EngineInstance, HostError},
    windows::user32::{
        User32Error32 as E,
        images::{
            ImageDirectory32, ImageExtent32, ImageKind32, ImageSource32, LoadImage32,
            cursor_resource,
        },
    },
};

pub(super) struct ResourcePixels32 {
    pub source: ResourceImage32,
    pub(super) bytes: Vec<u8>,
    pub(super) offset: usize,
    pub hotspot: (u16, u16),
    pub extent: ImageExtent32,
}
impl EngineInstance {
    pub(super) fn read_user32_image_resource(
        &self,
        instance: u32,
        name: Id,
        request: LoadImage32,
        defaults: ImageExtent32,
        budget: usize,
    ) -> Result<ResourcePixels32, HostError> {
        let payload = self.resource_data_payload(
            instance,
            &Id::Id(u32::from(request.kind.group_type())),
            &name,
            0,
            budget,
        )?;
        let source = ResourceImage32 {
            module: payload.module,
            group: payload.entry,
            kind: request.kind,
            name,
            language: payload.language,
        };
        self.decode_user32_image_resource(source, payload.bytes, request, defaults, budget)
    }

    pub(super) fn read_user32_image_module(
        &self,
        source: ResourceImage32,
        request: LoadImage32,
        defaults: ImageExtent32,
        budget: usize,
    ) -> Result<ResourcePixels32, HostError> {
        let payload = self.resource_payload_for_module(
            source.module,
            &Id::Id(u32::from(request.kind.group_type())),
            &source.name,
            source.language,
            budget,
        )?;
        if payload.entry != source.group || payload.language != source.language {
            return Err(HostError::User32(E::Win32(1814)));
        }
        self.decode_user32_image_resource(source, payload.bytes, request, defaults, budget)
    }

    fn decode_user32_image_resource(
        &self,
        source: ResourceImage32,
        bytes: Vec<u8>,
        request: LoadImage32,
        defaults: ImageExtent32,
        budget: usize,
    ) -> Result<ResourcePixels32, HostError> {
        if request.kind == ImageKind32::Bitmap {
            return Ok(ResourcePixels32 {
                source,
                bytes,
                offset: 0,
                hotspot: (0, 0),
                extent: ImageExtent32 {
                    width: request.width,
                    height: request.height,
                },
            });
        }
        let group = ImageDirectory32::resource(&bytes, request.kind).map_err(HostError::User32)?;
        let entry = group
            .select(
                request.selection(group.first().extent, defaults),
                if request.monochrome() { 1 } else { 32 },
            )
            .map_err(HostError::User32)?;
        let ImageSource32::Resource(id) = entry.source else {
            unreachable!()
        };
        let image = self.resource_payload_for_module(
            source.module,
            &Id::Id(u32::from(request.kind.resource_type())),
            &Id::Id(u32::from(id)),
            source.language,
            budget,
        )?;
        if image.bytes.len() != entry.bytes as usize {
            return Err(HostError::User32(E::Win32(13)));
        }
        let (hotspot, offset) = if request.kind == ImageKind32::Cursor {
            let (hotspot, _) = cursor_resource(&image.bytes).map_err(HostError::User32)?;
            (hotspot, 4)
        } else {
            ((0, 0), 0)
        };
        Ok(ResourcePixels32 {
            source,
            bytes: image.bytes,
            offset,
            hotspot,
            extent: request.requested_extent(defaults),
        })
    }
}
