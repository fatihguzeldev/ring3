use super::{
    DSoundRuntime32,
    lifetime::Publication32,
    memory::{output, wave, words, write_words},
    objects::{CaptureBuffer32, ObjectKind32},
};
use crate::{
    audio::{
        AudioContext,
        dsound::{CaptureRing, HResult, InterfaceKind},
    },
    memory::{AddressSpace, GuestAddress},
    windows::synchronization::SynchronizationContext32,
};
impl DSoundRuntime32 {
    pub(in crate::process::windows::dsound) fn capture_device_call(
        &mut self,
        memory: &mut AddressSpace,
        audio: &mut AudioContext,
        pointer: GuestAddress,
        slot: u8,
        args: &[u32],
    ) -> Result<u32, HResult> {
        let ObjectKind32::CaptureDevice { version8, .. } = &self.object(pointer)?.kind else {
            return Err(HResult::INVALID_PARAM);
        };
        let version8 = *version8;
        match slot {
            3 => {
                output(memory, args[2], 4)?;
                if args[3] != 0 {
                    return Err(HResult::NO_AGGREGATION);
                }
                if !self.config.capture_available {
                    return Err(HResult::NO_DRIVER);
                }
                let header = words::<5>(memory, args[1])?;
                if !matches!(header[0], 20 | 28) || header[3] != 0 {
                    return Err(HResult::INVALID_PARAM);
                }
                if header[0] == 28 {
                    let effects = words::<2>(
                        memory,
                        args[1].checked_add(20).ok_or(HResult::INVALID_PARAM)?,
                    )?;
                    if effects[0] != 0 {
                        return Err(HResult::FX_UNAVAILABLE);
                    }
                }
                let format = wave(memory, header[4])?;
                let retained = self
                    .records
                    .iter()
                    .flatten()
                    .map(|object| {
                        if let ObjectKind32::CaptureBuffer(buffer) = &object.kind {
                            buffer.native.bytes().len() as u32
                        } else {
                            0
                        }
                    })
                    .sum::<u32>();
                let budget = self
                    .config
                    .max_capture_bytes
                    .checked_sub(retained)
                    .ok_or(HResult::OUT_OF_MEMORY)?;
                let native = CaptureRing::new(format, header[2], header[1], budget)?;
                let device = self.object(pointer)?.pointer;
                let interfaces = if version8 {
                    &[
                        InterfaceKind::CaptureBuffer,
                        InterfaceKind::CaptureBuffer8,
                        InterfaceKind::Notify,
                    ][..]
                } else {
                    &[InterfaceKind::CaptureBuffer, InterfaceKind::Notify][..]
                };
                self.publish_object(
                    memory,
                    audio,
                    ObjectKind32::CaptureBuffer(CaptureBuffer32 {
                        native,
                        device,
                        version8,
                        notifications: Vec::new(),
                    }),
                    Publication32 {
                        interfaces,
                        output: Some((args[2], InterfaceKind::CaptureBuffer)),
                        retain: &[device],
                    },
                )?;
                Ok(0)
            }
            4 => {
                if words::<1>(memory, args[1])?[0] != 16 {
                    return Err(HResult::INVALID_PARAM);
                }
                write_words(memory, args[1], &[16, 32, 0xffff, 2])?;
                Ok(0)
            }
            5 => self.initialize_device(memory, pointer, args[1], true),
            _ => Err(HResult::UNSUPPORTED),
        }
    }
    pub(in crate::process::windows::dsound) fn capture_buffer_call(
        &mut self,
        memory: &mut AddressSpace,
        audio: &mut AudioContext,
        sync: Option<&mut SynchronizationContext32>,
        pointer: GuestAddress,
        slot: u8,
        args: &[u32],
    ) -> Result<u32, HResult> {
        if slot == 8 {
            return self.lock_buffer(memory, audio, pointer, args);
        }
        if slot == 11 {
            return self.unlock_buffer(memory, audio, pointer, args);
        }
        let index = self.slot(pointer)?;
        let ObjectKind32::CaptureBuffer(buffer) = &self.records[index].as_ref().unwrap().kind
        else {
            return Err(HResult::INVALID_PARAM);
        };
        match slot {
            3 => {
                if words::<1>(memory, args[1])?[0] != 16 {
                    return Err(HResult::INVALID_PARAM);
                }
                write_words(
                    memory,
                    args[1],
                    &[
                        16,
                        buffer.native.flags,
                        buffer.native.bytes().len() as u32,
                        0,
                    ],
                )?;
            }
            4 => {
                let position = buffer.native.position().to_le_bytes();
                let mut spans = [(GuestAddress(0), &position[..]); 2];
                let mut count = 0;
                for address in [args[1], args[2]] {
                    if address != 0 {
                        output(memory, address, 4)?;
                        spans[count] = (GuestAddress(address), &position);
                        count += 1;
                    }
                }
                if count == 0 {
                    return Err(HResult::INVALID_PARAM);
                }
                memory.write_spans(&spans[..count])?;
            }
            5 => {
                return super::buffers::properties::get_format(
                    memory,
                    buffer.native.format,
                    args[1],
                    args[2],
                    args[3],
                );
            }
            6 => write_words(memory, args[1], &[buffer.native.status()])?,
            7 => return Err(HResult::ALREADY_INITIALIZED),
            9 => {
                if !self.config.capture_available {
                    return Err(HResult::NO_DRIVER);
                }
                let ObjectKind32::CaptureBuffer(buffer) =
                    &mut self.records[index].as_mut().unwrap().kind
                else {
                    unreachable!()
                };
                buffer.native.start(args[1])?;
            }
            10 => {
                let playing = buffer.native.status() & 1 != 0;
                let prepared = if playing {
                    let signals = self.stop_signals(pointer)?;
                    if signals.is_empty() {
                        None
                    } else {
                        Some(
                            sync.as_deref()
                                .ok_or(HResult::INVALID_CALL)?
                                .prepare_signal_occurrences(signals)
                                .map_err(|_| HResult::INVALID_CALL)?,
                        )
                    }
                } else {
                    None
                };
                let ObjectKind32::CaptureBuffer(buffer) =
                    &mut self.records[index].as_mut().unwrap().kind
                else {
                    unreachable!()
                };
                buffer.native.stop()?;
                if let Some(prepared) = prepared {
                    sync.unwrap().commit_signals(prepared);
                }
            }
            12 => return Err(HResult::OBJECT_NOT_FOUND),
            13 => {
                if args[1] != 0 {
                    return Err(HResult::INVALID_PARAM);
                }
            }
            _ => return Err(HResult::UNSUPPORTED),
        }
        Ok(0)
    }
}
