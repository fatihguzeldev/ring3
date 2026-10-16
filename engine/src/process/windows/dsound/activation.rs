use super::{
    DSoundRuntime32,
    device::validate_guid,
    lifetime::Publication32,
    objects::{Device32, ObjectKind32},
};
use crate::{
    audio::{
        AudioContext,
        dsound::{Device, HResult, InterfaceKind},
    },
    cpu::dbt::GateSpec,
    memory::{AddressSpace, GuestAddress},
    windows::{
        com::{IID_IUNKNOWN, InterfaceId},
        ole32::KnownClass32,
    },
};
impl DSoundRuntime32 {
    pub(in crate::process) fn activate_class(
        &mut self,
        memory: &mut AddressSpace,
        audio: &mut AudioContext,
        gates: &[GateSpec],
        class: KnownClass32,
        iid: InterfaceId,
        out: u32,
    ) -> Result<u32, HResult> {
        let (kind, interfaces): (ObjectKind32, &[InterfaceKind]) = match class {
            KnownClass32::DirectSound => (
                ObjectKind32::Device(Device32 {
                    initialized: false,
                    native: Device::new(false, audio.config().sample_rate),
                    primary: None,
                }),
                &[InterfaceKind::Device],
            ),
            KnownClass32::DirectSound8 => (
                ObjectKind32::Device(Device32 {
                    initialized: false,
                    native: Device::new(true, audio.config().sample_rate),
                    primary: None,
                }),
                &[InterfaceKind::Device, InterfaceKind::Device8],
            ),
            KnownClass32::DirectSoundCapture | KnownClass32::DirectSoundCapture8 => (
                ObjectKind32::CaptureDevice {
                    initialized: false,
                    version8: class == KnownClass32::DirectSoundCapture8,
                },
                &[InterfaceKind::Capture],
            ),
            KnownClass32::DirectSoundFullDuplex => (
                ObjectKind32::FullDuplex {
                    device: None,
                    capture: None,
                },
                &[InterfaceKind::FullDuplex],
            ),
            _ => return Err(HResult::NO_INTERFACE),
        };
        let requested = if iid == IID_IUNKNOWN {
            InterfaceKind::Unknown
        } else {
            interfaces
                .iter()
                .copied()
                .find(|interface| interface.iid() == iid)
                .ok_or(HResult::NO_INTERFACE)?
        };
        self.initialize_methods(gates)?;
        self.publish_object(
            memory,
            audio,
            kind,
            Publication32 {
                interfaces,
                output: Some((out, requested)),
                retain: &[],
            },
        )?;
        Ok(0)
    }
    pub(in crate::process::windows::dsound) fn initialize_device(
        &mut self,
        memory: &AddressSpace,
        pointer: GuestAddress,
        guid: u32,
        capture: bool,
    ) -> Result<u32, HResult> {
        let initialized = match &self.object(pointer)?.kind {
            ObjectKind32::Device(device) => device.initialized,
            ObjectKind32::CaptureDevice { initialized, .. } => *initialized,
            _ => return Err(HResult::INVALID_PARAM),
        };
        if initialized {
            return Err(HResult::ALREADY_INITIALIZED);
        }
        validate_guid(memory, guid, capture)?;
        if capture && !self.config.capture_available {
            return Err(HResult::NO_DRIVER);
        }
        let index = self.slot(pointer)?;
        match &mut self.records[index].as_mut().unwrap().kind {
            ObjectKind32::Device(device) => device.initialized = true,
            ObjectKind32::CaptureDevice { initialized, .. } => *initialized = true,
            _ => unreachable!(),
        }
        Ok(0)
    }
}
