use super::super::{D3D8Runtime32, DispatchError32};
use super::decoding::copy_regions;
use crate::{
    graphics::d3d8::api::{HResult, InterfaceKind},
    memory::{AddressSpace, GuestAddress},
};

impl D3D8Runtime32 {
    pub(in crate::process::windows::d3d8) fn transfer_call(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        slot: u8,
        args: &[u32],
    ) -> Result<Option<u32>, DispatchError32> {
        match slot {
            28 => {
                if self.object(GuestAddress(args[1]))?.interface != InterfaceKind::Surface
                    || self.object(GuestAddress(args[4]))?.interface != InterfaceKind::Surface
                {
                    return Err(HResult::INVALID_CALL.into());
                }
                let source = self.resource(GuestAddress(args[1]), pointer)?;
                let destination = self.resource(GuestAddress(args[4]), pointer)?;
                let maximum = self.device(pointer)?.queue().limits().pending_bytes;
                let (rectangles, points) =
                    copy_regions(memory, args[2], args[5], args[3], maximum)?;
                let outcome = self.device_mut(pointer)?.copy_rects(
                    source,
                    destination,
                    &rectangles,
                    points.as_deref(),
                )?;
                if let Some(id) = outcome.value {
                    return Err(DispatchError32::Readback { id });
                }
            }
            29 => {
                let source = self.resource(GuestAddress(args[1]), pointer)?;
                let destination = self.resource(GuestAddress(args[2]), pointer)?;
                self.device_mut(pointer)?
                    .update_texture(source.resource, destination.resource)?;
            }
            30 => {
                if self.object(GuestAddress(args[1]))?.interface != InterfaceKind::Surface {
                    return Err(HResult::INVALID_CALL.into());
                }
                let destination = self.resource(GuestAddress(args[1]), pointer)?;
                let mode = match &self.object(pointer)?.kind {
                    super::super::objects::ObjectKind32::Device(owner) => {
                        owner.presentation.display_mode(&self.config.adapter)?
                    }
                    _ => return Err(HResult::INVALID_CALL.into()),
                };
                let outcome = self.device_mut(pointer)?.get_front_buffer_extent(
                    destination,
                    mode.width,
                    mode.height,
                )?;
                return Err(DispatchError32::Readback { id: outcome.value });
            }
            _ => return Ok(None),
        }
        Ok(Some(0))
    }
}
