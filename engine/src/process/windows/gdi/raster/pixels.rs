use super::super::{bitmaps::bounds, context::GdiRuntime32, storage::Storage32};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::{
        gdi::{GdiApi32, GdiError32},
        user32::{Point32, Rect32},
    },
};
impl GdiRuntime32 {
    pub(in crate::process::windows::gdi) fn pixel(
        &mut self,
        memory: &mut AddressSpace,
        api: GdiApi32,
        args: &[u32],
    ) -> Result<u32, GdiError32> {
        let index = self.dc_surface(args[0])?;
        let surface = &mut self.surfaces[index];
        let state = self.model.state(args[0])?;
        let point = state.device_point(Point32 {
            x: args[1] as i32,
            y: args[2] as i32,
        })?;
        if !state.accepts(point, bounds(surface.layout)) {
            return Ok(if api == GdiApi32::SetPixelV {
                0
            } else {
                u32::MAX
            });
        }
        surface.storage.check(memory)?;
        let (offset, count, local_x) = surface.layout.pixel_span(point.x as u32, point.y as u32)?;
        let layout = crate::windows::gdi::BitmapLayout32 {
            height: 1,
            top_down: true,
            ..surface.layout
        };
        let mut bytes = [0; 4];
        let address = match &surface.storage {
            Storage32::Owned(input) => {
                bytes[..count].copy_from_slice(&input[offset..offset + count]);
                None
            }
            Storage32::Dib { address, .. } => {
                let address = GuestAddress(
                    address
                        .checked_add(offset as u32)
                        .ok_or(GdiError32::InvalidRequest)?,
                );
                memory.read(address, &mut bytes[..count])?;
                Some(address)
            }
        };
        if api == GdiApi32::GetPixel {
            return Ok(layout.read_pixel(&bytes, &surface.palette, local_x, 0));
        }
        let revision = surface
            .revision
            .checked_add(1)
            .ok_or(GdiError32::Exhausted)?;
        layout.write_pixel(&mut bytes, &surface.palette, local_x, 0, args[3]);
        let color = layout.read_pixel(&bytes, &surface.palette, local_x, 0);
        if let Some(address) = address {
            memory.write(address, &bytes[..count])?;
        } else if let Storage32::Owned(output) = &mut surface.storage {
            output[offset..offset + count].copy_from_slice(&bytes[..count]);
        }
        surface.revision = revision;
        let damage = Rect32 {
            left: point.x,
            top: point.y,
            right: point.x + 1,
            bottom: point.y + 1,
        };
        surface.damage = if surface.damage.left >= surface.damage.right
            || surface.damage.top >= surface.damage.bottom
        {
            damage
        } else {
            Rect32 {
                left: surface.damage.left.min(damage.left),
                top: surface.damage.top.min(damage.top),
                right: surface.damage.right.max(damage.right),
                bottom: surface.damage.bottom.max(damage.bottom),
            }
        };
        Ok(if api == GdiApi32::SetPixelV { 1 } else { color })
    }
}
