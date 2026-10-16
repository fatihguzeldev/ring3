use super::{DSoundRuntime32, lifetime::group, objects::ObjectKind32};
use crate::{
    audio::{
        AudioContext,
        dsound::{FactoryGate, HResult, InterfaceKind, MethodGate},
    },
    memory::{AddressSpace, GuestAddress},
    process::WindowsProviderOutcome32,
    windows::{synchronization::SynchronizationContext32, window::WindowContext32},
};
impl DSoundRuntime32 {
    pub(in crate::process::windows::dsound) fn factory_call(
        &mut self,
        memory: &mut AddressSpace,
        audio: &mut AudioContext,
        windows: Option<&WindowContext32>,
        gate: FactoryGate,
        args: &[u32],
    ) -> Result<WindowsProviderOutcome32, HResult> {
        let result = match gate {
            FactoryGate::Create
            | FactoryGate::Create8
            | FactoryGate::CaptureCreate
            | FactoryGate::CaptureCreate8 => self.create_device(memory, audio, gate, args)?,
            FactoryGate::EnumerateA
            | FactoryGate::EnumerateW
            | FactoryGate::CaptureEnumerateA
            | FactoryGate::CaptureEnumerateW => {
                return self.enumerate(
                    memory,
                    args[0],
                    args[1],
                    matches!(
                        gate,
                        FactoryGate::CaptureEnumerateA | FactoryGate::CaptureEnumerateW
                    ),
                    matches!(
                        gate,
                        FactoryGate::EnumerateW | FactoryGate::CaptureEnumerateW
                    ),
                );
            }
            FactoryGate::GetDeviceId => self.get_device_id(memory, args[0], args[1])?,
            FactoryGate::FullDuplexCreate8 => {
                self.create_full_duplex(memory, audio, windows, args)?
            }
        };
        Ok(WindowsProviderOutcome32::Return(result))
    }
    #[allow(clippy::too_many_arguments)]
    pub(in crate::process::windows::dsound) fn method_call(
        &mut self,
        memory: &mut AddressSpace,
        audio: &mut AudioContext,
        sync: Option<&mut SynchronizationContext32>,
        windows: Option<&WindowContext32>,
        gate: MethodGate,
        args: &[u32],
    ) -> Result<u32, HResult> {
        let pointer = GuestAddress(args[0]);
        self.com.preflight(memory, pointer)?;
        if self.target(memory, pointer, gate.interface)? != pointer
            || self.object(pointer)?.counts[group(gate.interface)] == 0
        {
            return Err(HResult::INVALID_CALL);
        }
        match gate.slot {
            0 => return self.query_interface(memory, pointer, args[1], args[2]),
            1 => return self.add_ref(memory, pointer, gate.interface),
            2 => return self.release(memory, audio, sync, pointer, gate.interface),
            _ => {}
        }
        match &self.object(pointer)?.kind {
            ObjectKind32::Device(device) if !device.initialized && gate.slot != 10 => {
                return Err(HResult::UNINITIALIZED);
            }
            ObjectKind32::CaptureDevice {
                initialized: false, ..
            } if gate.slot != 5 => return Err(HResult::UNINITIALIZED),
            _ => {}
        }
        match gate.interface {
            InterfaceKind::Device | InterfaceKind::Device8 => self.device_call(
                memory,
                audio,
                super::device::DeviceEnvironment32 { sync, windows },
                pointer,
                gate.slot,
                args,
            ),
            InterfaceKind::Buffer | InterfaceKind::Buffer8 => {
                self.buffer_call(memory, audio, sync, pointer, gate.slot, args)
            }
            InterfaceKind::Notify => self.set_notifications(
                memory,
                audio,
                sync.ok_or(HResult::INVALID_CALL)?,
                pointer,
                args[1],
                args[2],
            ),
            InterfaceKind::Listener | InterfaceKind::Buffer3D => self.spatial_call(
                memory,
                audio,
                sync,
                pointer,
                gate.interface,
                gate.slot,
                args,
            ),
            InterfaceKind::Capture => {
                self.capture_device_call(memory, audio, pointer, gate.slot, args)
            }
            InterfaceKind::CaptureBuffer | InterfaceKind::CaptureBuffer8 => {
                self.capture_buffer_call(memory, audio, sync, pointer, gate.slot, args)
            }
            InterfaceKind::FullDuplex => {
                self.initialize_full_duplex(memory, audio, windows, pointer, args)
            }
            InterfaceKind::Unknown => Err(HResult::UNSUPPORTED),
        }
    }
}
