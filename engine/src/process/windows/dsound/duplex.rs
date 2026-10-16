use super::{
    DSoundRuntime32,
    device::validate_guid,
    lifetime::Publication32,
    memory::{output, read, wave, word, words},
    objects::{Buffer32, CaptureBuffer32, Device32, ObjectKind32},
};
use crate::{
    audio::{
        AudioContext,
        dsound::{Buffer, BufferDescription, CaptureRing, Device, HResult, InterfaceKind},
    },
    memory::{AddressSpace, GuestAddress},
    windows::window::{WindowContext32, WindowHandle32},
};
impl DSoundRuntime32 {
    pub(in crate::process::windows::dsound) fn create_full_duplex(
        &mut self,
        memory: &mut AddressSpace,
        audio: &mut AudioContext,
        windows: Option<&WindowContext32>,
        args: &[u32],
    ) -> Result<u32, HResult> {
        self.create_full_duplex_inner(memory, audio, windows, args, None)
    }
    pub(in crate::process::windows::dsound) fn initialize_full_duplex(
        &mut self,
        memory: &mut AddressSpace,
        audio: &mut AudioContext,
        windows: Option<&WindowContext32>,
        pointer: GuestAddress,
        args: &[u32],
    ) -> Result<u32, HResult> {
        let ObjectKind32::FullDuplex { device, .. } = &self.object(pointer)?.kind else {
            return Err(HResult::INVALID_PARAM);
        };
        if device.is_some() {
            return Err(HResult::ALREADY_INITIALIZED);
        }
        let arguments = [
            args[1], args[2], args[3], args[4], args[5], args[6], 0, args[7], args[8], 0,
        ];
        self.create_full_duplex_inner(memory, audio, windows, &arguments, Some(pointer))
    }
    fn create_full_duplex_inner(
        &mut self,
        memory: &mut AddressSpace,
        audio: &mut AudioContext,
        windows: Option<&WindowContext32>,
        args: &[u32],
        existing: Option<GuestAddress>,
    ) -> Result<u32, HResult> {
        for address in &args[if existing.is_some() { 7 } else { 6 }..9] {
            output(memory, *address, 4)?;
        }
        if args[9] != 0 {
            return Err(HResult::NO_AGGREGATION);
        }
        if !self.config.capture_available {
            return Err(HResult::NO_DRIVER);
        }
        validate_guid(memory, args[0], true)?;
        validate_guid(memory, args[1], false)?;
        let window = WindowHandle32::try_from(args[4]).map_err(|_| HResult::INVALID_PARAM)?;
        windows
            .ok_or(HResult::INVALID_PARAM)?
            .get(window)
            .map_err(|_| HResult::INVALID_PARAM)?;
        let mut device = Device::new(true, audio.config().sample_rate);
        device.set_cooperative_level(args[4], args[5])?;
        let playback = words::<5>(memory, args[3])?;
        if !matches!(playback[0], 20 | 36) || playback[3] != 0 {
            return Err(HResult::INVALID_PARAM);
        }
        let description = BufferDescription {
            flags: playback[1],
            bytes: playback[2],
            format: if playback[4] == 0 {
                None
            } else {
                Some(wave(memory, playback[4])?)
            },
            algorithm: if playback[0] == 36 {
                read(
                    memory,
                    args[3].checked_add(20).ok_or(HResult::INVALID_PARAM)?,
                )?
            } else {
                [0; 16]
            },
        }
        .validate(true)?;
        let creation_result = description.spatial_algorithm()?.creation_result().0;
        if description.flags & BufferDescription::PRIMARY != 0 {
            return Err(HResult::INVALID_PARAM);
        }
        let capture = words::<5>(memory, args[2])?;
        if !matches!(capture[0], 20 | 28) || capture[3] != 0 {
            return Err(HResult::INVALID_PARAM);
        }
        let format = wave(memory, capture[4])?;
        let effects =
            self.prepare_capture_effects(memory, args[2], format, true, true, capture[2])?;
        let allocated =
            self.capture_allocation_bytes() + effects.as_ref().map_or(0, |effects| effects.bytes());
        let capture = CaptureRing::new(
            format,
            capture[2],
            capture[1],
            self.config
                .max_capture_bytes
                .checked_sub(allocated)
                .ok_or(HResult::OUT_OF_MEMORY)?,
        )?;
        if self
            .records
            .iter()
            .filter(|record| record.is_none())
            .count()
            < if existing.is_some() { 4 } else { 5 }
            || memory.transaction_version().checked_add(11).is_none()
        {
            return Err(HResult::OUT_OF_MEMORY);
        }
        let mut created = Vec::new();
        created
            .try_reserve_exact(5)
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        let native = Buffer::create(audio, &device, description)?;
        native.apply_controls(audio, &device)?;
        let reference = if effects.is_some() {
            match audio.prepare_capture_reference() {
                Ok(reference) => reference,
                Err(error) => {
                    audio.release_stream_voice(native.voice).unwrap();
                    audio.release_stream_buffer(native.storage).unwrap();
                    return Err(error.into());
                }
            }
        } else {
            None
        };
        let mut native = Some(native);
        let mut capture = Some(capture);
        let mut effects = effects;
        let result = (|| {
            let device = self.publish_object(
                memory,
                audio,
                ObjectKind32::Device(Device32 {
                    initialized: true,
                    native: device,
                    primary: None,
                }),
                Publication32 {
                    interfaces: &[InterfaceKind::Device, InterfaceKind::Device8],
                    output: None,
                    retain: &[],
                },
            )?;
            created.push(device);
            let input = self.publish_object(
                memory,
                audio,
                ObjectKind32::CaptureDevice {
                    version8: true,
                    initialized: true,
                },
                Publication32 {
                    interfaces: &[InterfaceKind::Capture],
                    output: None,
                    retain: &[],
                },
            )?;
            created.push(input);
            let mut interfaces = [
                InterfaceKind::Buffer,
                InterfaceKind::Buffer8,
                InterfaceKind::Notify,
                InterfaceKind::Buffer3D,
            ];
            let mut count = 2;
            if description.flags & BufferDescription::NOTIFY != 0 {
                interfaces[count] = InterfaceKind::Notify;
                count += 1;
            }
            if description.flags & BufferDescription::CONTROL_3D != 0 {
                interfaces[count] = InterfaceKind::Buffer3D;
                count += 1;
            }
            let render = self.publish_object(
                memory,
                audio,
                ObjectKind32::Buffer(Buffer32 {
                    effects_generation: 0,
                    native: native.take().unwrap(),
                    device,
                    version8: true,
                    notifications: Vec::new(),
                }),
                Publication32 {
                    interfaces: &interfaces[..count],
                    output: None,
                    retain: &[device],
                },
            )?;
            created.push(render);
            let reference_object = effects.as_ref().map(|_| render);
            let retained = [input, render];
            let record = self.publish_object(
                memory,
                audio,
                ObjectKind32::CaptureBuffer(CaptureBuffer32 {
                    native: capture.take().unwrap(),
                    device: input,
                    version8: true,
                    notifications: Vec::new(),
                    effects: effects.take(),
                    reference: reference_object,
                    reference_origin: 0,
                }),
                Publication32 {
                    interfaces: &[
                        InterfaceKind::CaptureBuffer,
                        InterfaceKind::CaptureBuffer8,
                        InterfaceKind::Notify,
                    ],
                    output: None,
                    retain: &retained[..if reference_object.is_some() { 2 } else { 1 }],
                },
            )?;
            created.push(record);
            let duplex = if let Some(pointer) = existing {
                pointer
            } else {
                let duplex = self.publish_object(
                    memory,
                    audio,
                    ObjectKind32::FullDuplex {
                        device: Some(device),
                        capture: Some(input),
                    },
                    Publication32 {
                        interfaces: &[InterfaceKind::FullDuplex],
                        output: None,
                        retain: &[],
                    },
                )?;
                created.push(duplex);
                duplex
            };
            let mut writes = [
                word(
                    args[7],
                    self.target(memory, record, InterfaceKind::CaptureBuffer8)?
                        .0,
                ),
                word(
                    args[8],
                    self.target(memory, render, InterfaceKind::Buffer8)?.0,
                ),
                word(
                    if existing.is_none() { args[6] } else { args[7] },
                    self.target(memory, duplex, InterfaceKind::FullDuplex)?.0,
                ),
            ];
            let count = if existing.is_none() { 3 } else { 2 };
            writes[..count].sort_unstable_by_key(|write| write.address.0);
            memory.write_words32(&writes[..count])?;
            if existing.is_some() {
                let slot = self.slot(duplex)?;
                self.records[slot].as_mut().unwrap().kind = ObjectKind32::FullDuplex {
                    device: Some(device),
                    capture: Some(input),
                };
            }
            Ok(creation_result)
        })();
        if result.is_err() {
            if let Some(native) = native {
                audio.release_stream_voice(native.voice).unwrap();
                audio.release_stream_buffer(native.storage).unwrap();
            }
            self.rollback_duplex(memory, audio, &created);
        } else {
            audio.commit_capture_reference(reference);
        }
        result
    }
    fn rollback_duplex(
        &mut self,
        memory: &mut AddressSpace,
        audio: &mut AudioContext,
        created: &[GuestAddress],
    ) {
        for &pointer in created.iter().rev() {
            let index = self.slot(pointer).unwrap();
            let object = self.records[index].take().unwrap();
            match &object.kind {
                ObjectKind32::Buffer(buffer) => {
                    audio.release_stream_voice(buffer.native.voice).unwrap();
                    let _ = audio.release_stream_buffer(buffer.native.storage);
                    self.com.release(memory, buffer.device).unwrap();
                }
                ObjectKind32::CaptureBuffer(buffer) => {
                    self.com.release(memory, buffer.device).unwrap();
                    if let Some(reference) = buffer.reference {
                        self.com.release(memory, reference).unwrap();
                    }
                }
                ObjectKind32::FullDuplex { .. } => {}
                _ => {}
            }
            self.com
                .release(memory, pointer)
                .expect("duplex rollback mapping versions were reserved");
        }
    }
}
