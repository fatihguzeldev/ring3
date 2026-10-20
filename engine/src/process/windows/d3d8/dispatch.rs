use super::{
    D3D8Runtime32, DispatchError32,
    fpu::FpuSetup32,
    memory::{check_output, write_words},
    objects::ObjectKind32,
};
use crate::{
    graphics::d3d8::{
        api::{HResult, InterfaceKind, MethodGate},
        device::DeviceStatus,
    },
    memory::{AddressSpace, GuestAddress},
    windows::window::WindowContext32,
};

impl D3D8Runtime32 {
    pub(super) fn method_call(
        &mut self,
        memory: &mut AddressSpace,
        windows: Option<&WindowContext32>,
        gate: MethodGate,
        args: &[u32],
        epoch: u64,
        fpu: &mut FpuSetup32,
    ) -> Result<u32, DispatchError32> {
        let pointer = GuestAddress(args[0]);
        self.com.preflight(memory, pointer)?;
        if !self.object(pointer)?.interface.supports(gate.interface) {
            return Err(HResult::INVALID_CALL.into());
        }
        match gate.slot {
            0 => return Ok(self.query_interface(memory, pointer, args[1], args[2])?.0),
            1 => return Ok(self.add_ref(memory, pointer)?),
            2 => return Ok(self.release(memory, pointer)?),
            _ => {}
        }
        match gate.interface {
            InterfaceKind::Direct3D => {
                Ok(self.direct3d_call(memory, windows, gate.slot, args, epoch, fpu)?)
            }
            InterfaceKind::Device => self.device_call(
                memory,
                windows.ok_or(HResult::INVALID_CALL)?,
                pointer,
                gate.slot,
                args,
            ),
            InterfaceKind::SwapChain => self
                .swap_chain_call(
                    memory,
                    windows.ok_or(HResult::INVALID_CALL)?,
                    pointer,
                    gate.slot,
                    args,
                )?
                .ok_or_else(|| HResult::INVALID_CALL.into()),
            interface => self
                .resource_call(memory, pointer, interface, gate.slot, args)?
                .ok_or_else(|| HResult::INVALID_CALL.into()),
        }
    }

    fn device_call(
        &mut self,
        memory: &mut AddressSpace,
        windows: &WindowContext32,
        pointer: GuestAddress,
        slot: u8,
        args: &[u32],
    ) -> Result<u32, DispatchError32> {
        let device = self.device(pointer)?;
        // pure devices retain internal state for command emission, but refuse
        // driver-state readback and cpu vertex processing through their api.
        if device.description().behavior & 0x10 != 0
            && matches!(
                slot,
                38 | 43 | 45 | 47 | 49 | 51 | 58 | 59 | 62 | 74 | 80 | 92
            )
        {
            return Err(HResult::INVALID_CALL.into());
        }
        if let Some(value) = self.state_call(memory, pointer, slot, args)? {
            return Ok(value);
        }
        if let Some(value) = self.binding_call(memory, pointer, slot, args)? {
            return Ok(value);
        }
        if let Some(value) = self.shader_call(memory, pointer, slot, args)? {
            return Ok(value);
        }
        if let Some(value) = self.device_resource_call(memory, pointer, slot, args)? {
            return Ok(value);
        }
        if let Some(value) = self.presentation_call(memory, windows, pointer, slot, args)? {
            return Ok(value);
        }
        if let Some(value) = self.draw_call(memory, pointer, slot, args)? {
            return Ok(value);
        }
        if let Some(value) = self.transfer_call(memory, pointer, slot, args)? {
            return Ok(value);
        }
        match slot {
            3 => Ok(match self.device(pointer)?.status() {
                DeviceStatus::Operational => 0,
                DeviceStatus::Resettable => HResult::DEVICE_NOT_RESET.0,
                _ => HResult::DEVICE_LOST.0,
            }),
            4 => Ok(self
                .config
                .device
                .graphics
                .resource_bytes
                .saturating_sub(self.device(pointer)?.resources().resource_bytes() as u32)),
            5 => Ok(0), // no separate managed GPU cache is retained by the engine.
            6 => {
                let ObjectKind32::Device(device) = &self.object(pointer)?.kind else {
                    return Err(HResult::INVALID_CALL.into());
                };
                let parent = device.parent;
                self.acquire_output(memory, parent, args[1])?;
                Ok(0)
            }
            7 => {
                write_words(memory, args[1], &self.config.adapter.caps())?;
                Ok(0)
            }
            8 => {
                let ObjectKind32::Device(device) = &self.object(pointer)?.kind else {
                    return Err(HResult::INVALID_CALL.into());
                };
                write_words(
                    memory,
                    args[1],
                    &device
                        .presentation
                        .display_mode(&self.config.adapter)?
                        .words(),
                )?;
                Ok(0)
            }
            9 => {
                let ObjectKind32::Device(device) = &self.object(pointer)?.kind else {
                    return Err(HResult::INVALID_CALL.into());
                };
                write_words(
                    memory,
                    args[1],
                    &[0, 1, device.focus_window, device.behavior],
                )?;
                Ok(0)
            }
            64 => {
                let passes = self.device(pointer)?.validate_device()?;
                write_words(memory, args[1], &[passes])?;
                Ok(0)
            }
            65 => {
                if args[2] == 0 || args[3] == 0 {
                    return Err(HResult::INVALID_CALL.into());
                }
                Ok(if args[1] == 4 { 1 } else { 0x8000_4005 })
            }
            66 => {
                let bytes = super::memory::read_bytes(memory, args[2], 1024)?;
                let mut entries = [[0; 4]; 256];
                for (entry, bytes) in entries.iter_mut().zip(bytes.chunks_exact(4)) {
                    entry.copy_from_slice(bytes);
                }
                self.device_mut(pointer)?
                    .set_palette_entries(args[1], &entries)?;
                Ok(0)
            }
            67 => {
                check_output(memory, args[2], 1024)?;
                let entries = self.device(pointer)?.palette_entries(args[1])?;
                let mut bytes = [0; 1024];
                for (output, entry) in bytes.chunks_exact_mut(4).zip(entries) {
                    output.copy_from_slice(entry);
                }
                memory.write(GuestAddress(args[2]), &bytes)?;
                Ok(0)
            }
            _ => Err(HResult::INVALID_CALL.into()),
        }
    }
}
