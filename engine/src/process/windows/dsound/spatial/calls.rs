use super::super::{
    DSoundRuntime32,
    memory::{output, words, write_words},
    objects::ObjectKind32,
};
use super::decoding;
use super::publication::SpatialPublication32;
use crate::{
    audio::{
        AudioContext,
        dsound::{HResult, InterfaceKind},
    },
    memory::{AddressSpace, GuestAddress},
    windows::synchronization::SynchronizationContext32,
};
impl DSoundRuntime32 {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::process::windows::dsound) fn spatial_call(
        &mut self,
        memory: &mut AddressSpace,
        audio: &mut AudioContext,
        sync: Option<&mut SynchronizationContext32>,
        pointer: GuestAddress,
        interface: InterfaceKind,
        slot: u8,
        args: &[u32],
    ) -> Result<u32, HResult> {
        let index = self.slot(pointer)?;
        let ObjectKind32::Buffer(buffer) = &self.records[index].as_ref().unwrap().kind else {
            return Err(HResult::INVALID_PARAM);
        };
        let device_pointer = buffer.device;
        if interface == InterfaceKind::Listener {
            let mut value = self.device(device_pointer)?.native.listener;
            match slot {
                3 => {
                    if words::<1>(memory, args[1])?[0] != 64 {
                        return Err(HResult::INVALID_PARAM);
                    }
                    write_words(memory, args[1], &decoding::listener_words(value))?;
                    return Ok(0);
                }
                4 | 5 | 8 => {
                    let value = match slot {
                        4 => value.distance_factor,
                        5 => value.doppler_factor,
                        _ => value.rolloff_factor,
                    };
                    write_words(memory, args[1], &[value.to_bits()])?;
                    return Ok(0);
                }
                6 => {
                    let encode = |vector| {
                        let mut bytes = [0; 12];
                        for (word, chunk) in decoding::bits(vector)
                            .into_iter()
                            .zip(bytes.chunks_exact_mut(4))
                        {
                            chunk.copy_from_slice(&word.to_le_bytes());
                        }
                        bytes
                    };
                    let a = encode(value.front);
                    let b = encode(value.top);
                    output(memory, args[1], 12)?;
                    output(memory, args[2], 12)?;
                    memory
                        .write_spans(&[(GuestAddress(args[1]), &a), (GuestAddress(args[2]), &b)])?;
                    return Ok(0);
                }
                7 | 9 => {
                    write_words(
                        memory,
                        args[1],
                        &decoding::bits(if slot == 7 {
                            value.position
                        } else {
                            value.velocity
                        }),
                    )?;
                    return Ok(0);
                }
                10 => value = decoding::listener(words(memory, args[1])?)?,
                11 => value.distance_factor = f32::from_bits(args[1]),
                12 => value.doppler_factor = f32::from_bits(args[1]),
                13 => {
                    value.front = decoding::vector(&args[1..4]);
                    value.top = decoding::vector(&args[4..7]);
                }
                14 => value.position = decoding::vector(&args[1..4]),
                15 => value.rolloff_factor = f32::from_bits(args[1]),
                16 => value.velocity = decoding::vector(&args[1..4]),
                17 => return self.commit_spatial(audio, sync, device_pointer),
                _ => return Err(HResult::UNSUPPORTED),
            }
            let apply = match slot {
                13 => args[7],
                14 | 16 => args[4],
                _ => args[2],
            };
            if apply > 1 {
                return Err(HResult::INVALID_PARAM);
            }
            let value = value.validate()?;
            let device_index = self.slot(device_pointer)?;
            let mut candidate = self.device(device_pointer)?.native;
            candidate.listener = value;
            let prepared = if apply == 0 {
                candidate.applied_listener = value;
                self.prepare_spatial_publication(
                    audio,
                    sync.as_deref(),
                    SpatialPublication32 {
                        device_pointer,
                        device: candidate,
                        deferred: false,
                        buffer: None,
                    },
                )?
            } else {
                None
            };
            let ObjectKind32::Device(device) =
                &mut self.records[device_index].as_mut().unwrap().kind
            else {
                unreachable!()
            };
            device.native = candidate;
            if apply == 0 {
                self.apply_device_controls(audio, device_pointer)
                    .expect("listener controls were preflighted");
                self.commit_spatial_stops(audio, sync, device_pointer, candidate, prepared);
            }
            Ok(0)
        } else {
            let mut value = buffer.native.spatial;
            match slot {
                3 => {
                    if words::<1>(memory, args[1])?[0] != 64 {
                        return Err(HResult::INVALID_PARAM);
                    }
                    write_words(memory, args[1], &decoding::buffer_words(value))?;
                    return Ok(0);
                }
                4 => {
                    let values = [
                        value.inside_cone.to_le_bytes(),
                        value.outside_cone.to_le_bytes(),
                    ];
                    output(memory, args[1], 4)?;
                    output(memory, args[2], 4)?;
                    memory.write_spans(&[
                        (GuestAddress(args[1]), &values[0]),
                        (GuestAddress(args[2]), &values[1]),
                    ])?;
                    return Ok(0);
                }
                5 | 10 | 11 => {
                    write_words(
                        memory,
                        args[1],
                        &decoding::bits(match slot {
                            5 => value.cone_orientation,
                            10 => value.position,
                            _ => value.velocity,
                        }),
                    )?;
                    return Ok(0);
                }
                6..=9 => {
                    write_words(
                        memory,
                        args[1],
                        &[match slot {
                            6 => value.outside_volume as u32,
                            7 => value.max_distance.to_bits(),
                            8 => value.min_distance.to_bits(),
                            _ => value.mode,
                        }],
                    )?;
                    return Ok(0);
                }
                12 => value = decoding::buffer(words(memory, args[1])?)?,
                13 => {
                    value.inside_cone = args[1];
                    value.outside_cone = args[2];
                }
                14 => value.cone_orientation = decoding::vector(&args[1..4]),
                15 => value.outside_volume = args[1] as i32,
                16 => value.max_distance = f32::from_bits(args[1]),
                17 => value.min_distance = f32::from_bits(args[1]),
                18 => value.mode = args[1],
                19 => value.position = decoding::vector(&args[1..4]),
                20 => value.velocity = decoding::vector(&args[1..4]),
                _ => return Err(HResult::UNSUPPORTED),
            }
            let apply = match slot {
                13 => args[3],
                14 | 19 | 20 => args[4],
                _ => args[2],
            };
            let mut candidate = buffer.native;
            candidate.set_spatial(value, apply)?;
            let device = self.device(device_pointer)?.native;
            let prepared = if apply == 0 {
                self.prepare_spatial_publication(
                    audio,
                    sync.as_deref(),
                    SpatialPublication32 {
                        device_pointer,
                        device,
                        deferred: false,
                        buffer: Some((index, candidate)),
                    },
                )?
            } else {
                None
            };
            let ObjectKind32::Buffer(buffer) = &mut self.records[index].as_mut().unwrap().kind
            else {
                unreachable!()
            };
            buffer.native = candidate;
            if apply == 0 {
                candidate
                    .apply_controls(audio, &device)
                    .expect("spatial controls were preflighted");
                self.commit_spatial_stops(audio, sync, device_pointer, device, prepared);
            }
            Ok(0)
        }
    }
    fn commit_spatial(
        &mut self,
        audio: &mut AudioContext,
        sync: Option<&mut SynchronizationContext32>,
        pointer: GuestAddress,
    ) -> Result<u32, HResult> {
        let index = self.slot(pointer)?;
        let mut device = self.device(pointer)?.native;
        device.applied_listener = device.listener;
        let prepared = self.prepare_spatial_publication(
            audio,
            sync.as_deref(),
            SpatialPublication32 {
                device_pointer: pointer,
                device,
                deferred: true,
                buffer: None,
            },
        )?;
        let ObjectKind32::Device(record) = &mut self.records[index].as_mut().unwrap().kind else {
            unreachable!()
        };
        record.native = device;
        for object in self.records.iter_mut().flatten() {
            if let ObjectKind32::Buffer(buffer) = &mut object.kind
                && buffer.device == pointer
            {
                buffer.native.applied_spatial = buffer.native.spatial;
                buffer
                    .native
                    .apply_controls(audio, &device)
                    .expect("deferred sound controls were preflighted");
            }
        }
        self.commit_spatial_stops(audio, sync, pointer, device, prepared);
        Ok(0)
    }
}
