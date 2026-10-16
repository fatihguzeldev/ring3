use super::{
    DSoundRuntime32,
    lifetime::Publication32,
    memory::{output, read, words, write_words},
    objects::{Device32, ObjectKind32},
};
use crate::{
    audio::{
        AudioContext,
        dsound::{Device, FactoryGate, HResult, InterfaceKind},
    },
    memory::{AddressSpace, GuestAddress},
    windows::window::{WindowContext32, WindowHandle32},
};

pub(super) const PLAYBACK_GUID: [u8; 16] = [
    0x52, 0x33, 0x41, 0x55, 0x44, 0x49, 0x4f, 0x32, 0x92, 0x10, 0x40, 0x10, 0x15, 0, 0, 1,
];
pub(super) const CAPTURE_GUID: [u8; 16] = [
    0x52, 0x33, 0x41, 0x55, 0x44, 0x49, 0x4f, 0x32, 0x92, 0x10, 0x40, 0x10, 0x15, 0, 0, 2,
];
impl DSoundRuntime32 {
    pub(in crate::process::windows::dsound) fn create_device(
        &mut self,
        memory: &mut AddressSpace,
        audio: &mut AudioContext,
        gate: FactoryGate,
        args: &[u32],
    ) -> Result<u32, HResult> {
        output(memory, args[1], 4)?;
        if args[2] != 0 {
            return Err(HResult::NO_AGGREGATION);
        }
        let capture = matches!(
            gate,
            FactoryGate::CaptureCreate | FactoryGate::CaptureCreate8
        );
        validate_guid(memory, args[0], capture)?;
        if capture && !self.config.capture_available {
            return Err(HResult::NO_DRIVER);
        }
        let version8 = matches!(gate, FactoryGate::Create8 | FactoryGate::CaptureCreate8);
        let kind = if capture {
            ObjectKind32::CaptureDevice {
                version8,
                initialized: true,
            }
        } else {
            ObjectKind32::Device(Device32 {
                initialized: true,
                native: Device::new(version8, audio.config().sample_rate),
                primary: None,
            })
        };
        let interfaces = if capture {
            &[InterfaceKind::Capture][..]
        } else if version8 {
            &[InterfaceKind::Device, InterfaceKind::Device8][..]
        } else {
            &[InterfaceKind::Device][..]
        };
        let interface = if capture {
            InterfaceKind::Capture
        } else if version8 {
            InterfaceKind::Device8
        } else {
            InterfaceKind::Device
        };
        self.publish_object(
            memory,
            audio,
            kind,
            Publication32 {
                interfaces,
                output: Some((args[1], interface)),
                retain: &[],
            },
        )?;
        Ok(0)
    }
    pub(in crate::process::windows::dsound) fn device_call(
        &mut self,
        memory: &mut AddressSpace,
        audio: &mut AudioContext,
        windows: Option<&WindowContext32>,
        pointer: GuestAddress,
        slot: u8,
        args: &[u32],
    ) -> Result<u32, HResult> {
        match slot {
            3 => self.create_buffer(memory, audio, pointer, args[1], args[2], args[3]),
            4 => {
                if words::<1>(memory, args[1])?[0] != 96 {
                    return Err(HResult::INVALID_PARAM);
                }
                let caps = self.device(pointer)?.native.caps();
                write_words(memory, args[1], &caps)?;
                Ok(0)
            }
            5 => self.duplicate_buffer(memory, audio, pointer, GuestAddress(args[1]), args[2]),
            6 => {
                let window =
                    WindowHandle32::try_from(args[1]).map_err(|_| HResult::INVALID_PARAM)?;
                windows
                    .ok_or(HResult::INVALID_PARAM)?
                    .get(window)
                    .map_err(|_| HResult::INVALID_PARAM)?;
                let index = self.slot(pointer)?;
                let ObjectKind32::Device(device) = &mut self.records[index].as_mut().unwrap().kind
                else {
                    return Err(HResult::INVALID_PARAM);
                };
                device.native.set_cooperative_level(args[1], args[2])?;
                let canonical = self.records[index].as_ref().unwrap().pointer;
                self.apply_device_controls(audio, canonical)?;
                Ok(0)
            }
            7 => Ok(0),
            8 => {
                write_words(
                    memory,
                    args[1],
                    &[self.device(pointer)?.native.speaker_config],
                )?;
                Ok(0)
            }
            9 => {
                let speakers = args[1] & 0xff;
                let geometry = args[1] >> 16;
                if !matches!(speakers, 2 | 4) || args[1] & 0xff00 != 0 || geometry > 180 {
                    return Err(HResult::CONTROL_UNAVAILABLE);
                }
                let index = self.slot(pointer)?;
                let ObjectKind32::Device(device) = &mut self.records[index].as_mut().unwrap().kind
                else {
                    return Err(HResult::INVALID_PARAM);
                };
                device.native.speaker_config = args[1];
                Ok(0)
            }
            10 => self.initialize_device(memory, pointer, args[1], false),
            11 => {
                write_words(memory, args[1], &[1])?;
                Ok(0)
            }
            _ => Err(HResult::UNSUPPORTED),
        }
    }
}
pub(in crate::process::windows::dsound) fn validate_guid(
    memory: &AddressSpace,
    pointer: u32,
    capture: bool,
) -> Result<(), HResult> {
    if pointer == 0 {
        return Ok(());
    }
    let guid = read::<16>(memory, pointer)?;
    if guid == [0; 16] || guid == if capture { CAPTURE_GUID } else { PLAYBACK_GUID } {
        return Ok(());
    }
    let default = u32::from_le_bytes(guid[..4].try_into().unwrap());
    if guid[4..]
        == [
            0x6d, 0x9c, 0xed, 0x47, 0xaa, 0xf1, 0x4d, 0xda, 0x8f, 0x2b, 0x5c, 3,
        ]
        && if capture {
            matches!(default, 0xdef00001 | 0xdef00003)
        } else {
            matches!(default, 0xdef00000 | 0xdef00002)
        }
    {
        Ok(())
    } else {
        Err(HResult::NO_DRIVER)
    }
}
