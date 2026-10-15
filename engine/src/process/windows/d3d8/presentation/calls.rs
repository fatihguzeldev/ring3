use super::super::{
    D3D8Runtime32, DispatchError32,
    memory::{check_output, read_bytes, word},
};
use super::decoding::{destination_rect, dirty_region, source_rect};
use crate::{
    graphics::d3d8::{
        api::{HResult, InterfaceKind},
        commands::SignedRect,
    },
    memory::{AddressSpace, GuestAddress},
    windows::window::WindowContext32,
};

impl D3D8Runtime32 {
    pub(in crate::process::windows::d3d8) fn presentation_call(
        &mut self,
        memory: &mut AddressSpace,
        windows: &WindowContext32,
        pointer: GuestAddress,
        slot: u8,
        args: &[u32],
    ) -> Result<Option<u32>, DispatchError32> {
        match slot {
            10 => {
                if self.object(GuestAddress(args[3]))?.interface != InterfaceKind::Surface {
                    return Err(HResult::INVALID_CALL.into());
                }
                let surface = self.resource(GuestAddress(args[3]), pointer)?;
                self.device_mut(pointer)?
                    .set_cursor_shape(surface, (args[1], args[2]))?;
            }
            11 => {
                self.device_mut(pointer)?.set_cursor_position(
                    args[1] as i32,
                    args[2] as i32,
                    args[3],
                )?;
            }
            12 => {
                return Ok(Some(u32::from(
                    self.device_mut(pointer)?.show_cursor(args[1] != 0)?.value,
                )));
            }
            13 => self.create_swap_chain(memory, windows, pointer, args[1], args[2])?,
            14 => self.reset_device(memory, windows, pointer, args[1])?,
            15 => {
                let description = self.device(pointer)?.description();
                let surface = self.device(pointer)?.back_buffer(0)?;
                self.present_guest(
                    memory,
                    windows,
                    pointer,
                    surface,
                    u64::from(pointer.0),
                    description.window,
                    description.swap_effect,
                    description.interval,
                    args,
                )?;
            }
            16 => {
                if args[2] != 0 {
                    return Err(HResult::INVALID_CALL.into());
                }
                let surface = self.device(pointer)?.back_buffer(args[1])?;
                self.acquire_surface(memory, pointer, pointer, surface, args[3])?;
            }
            17 => {
                check_output(memory, args[1], 8)?;
                return Err(HResult::NOT_AVAILABLE.into());
            }
            18 => {
                let bytes = read_bytes(memory, args[2], 1536)?;
                let ramp = std::array::from_fn(|channel| {
                    std::array::from_fn(|index| {
                        let offset = (channel * 256 + index) * 2;
                        u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
                    })
                });
                self.device_mut(pointer)?.set_gamma(args[1], &ramp)?;
            }
            19 => {
                check_output(memory, args[1], 1536)?;
                let mut bytes = [0; 1536];
                for (index, value) in self.device(pointer)?.gamma().iter().flatten().enumerate() {
                    bytes[index * 2..index * 2 + 2].copy_from_slice(&value.to_le_bytes());
                }
                memory.write(GuestAddress(args[1]), &bytes)?;
            }
            31 => self.set_targets_guest(memory, pointer, args[1], args[2])?,
            32 => {
                let surface = self.device(pointer)?.render_target();
                self.acquire_surface(memory, pointer, pointer, surface, args[1])?;
            }
            33 => {
                if let Some(surface) = self.device(pointer)?.depth_target() {
                    self.acquire_surface(memory, pointer, pointer, surface, args[1])?;
                } else {
                    memory.write_words32(&[word(args[1], 0)])?;
                    return Ok(Some(HResult::NOT_FOUND.0));
                }
            }
            66 => {
                let bytes = read_bytes(memory, args[2], 1024)?;
                let entries = std::array::from_fn(|index| {
                    bytes[index * 4..index * 4 + 4].try_into().unwrap()
                });
                self.device_mut(pointer)?
                    .set_palette_entries(args[1], &entries)?;
            }
            67 => {
                check_output(memory, args[2], 1024)?;
                let entries = self.device(pointer)?.palette_entries(args[1])?;
                let mut bytes = [0; 1024];
                for (index, entry) in entries.iter().enumerate() {
                    bytes[index * 4..index * 4 + 4].copy_from_slice(entry);
                }
                memory.write(GuestAddress(args[2]), &bytes)?;
            }
            _ => return Ok(None),
        }
        Ok(Some(0))
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::process::windows::d3d8) fn present_guest(
        &mut self,
        memory: &AddressSpace,
        windows: &WindowContext32,
        device: GuestAddress,
        surface: crate::graphics::d3d8::resources::SurfaceRef,
        chain: u64,
        default_window: u32,
        swap_effect: u32,
        interval: u32,
        args: &[u32],
    ) -> Result<(), DispatchError32> {
        let source = source_rect(memory, args[1])?;
        let destination: Option<SignedRect> = destination_rect(memory, args[2])?;
        let maximum = self.device(device)?.queue().limits().pending_bytes;
        let dirty = dirty_region(memory, args[4], maximum)?;
        let window = if args[3] == 0 {
            default_window
        } else {
            args[3]
        };
        windows
            .get(
                crate::windows::window::WindowHandle32::try_from(window)
                    .map_err(|_| HResult::INVALID_CALL)?,
            )
            .map_err(|_| HResult::INVALID_CALL)?;
        self.device_mut(device)?.present_surface(
            surface,
            chain,
            window,
            source,
            destination,
            &dirty,
            args[4] != 0,
            swap_effect,
            interval,
        )?;
        Ok(())
    }
}
