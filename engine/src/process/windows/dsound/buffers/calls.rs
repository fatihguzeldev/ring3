use super::{
    super::{
        DSoundRuntime32,
        memory::{output, words, write_words},
        objects::ObjectKind32,
    },
    properties::get_format,
};
use crate::{
    audio::{
        AudioContext,
        dsound::{BufferDescription, HResult},
    },
    memory::{AddressSpace, GuestAddress},
    windows::synchronization::SynchronizationContext32,
};
impl DSoundRuntime32 {
    pub(in crate::process::windows::dsound) fn buffer_call(
        &mut self,
        memory: &mut AddressSpace,
        audio: &mut AudioContext,
        sync: Option<&mut SynchronizationContext32>,
        pointer: GuestAddress,
        slot: u8,
        args: &[u32],
    ) -> Result<u32, HResult> {
        if slot == 11 {
            return self.lock_buffer(memory, audio, pointer, args);
        }
        if slot == 19 {
            return self.unlock_buffer(memory, audio, pointer, args);
        }
        if matches!(slot, 15..=17) {
            return self.buffer_control(audio, pointer, slot, args[1]);
        }
        if slot == 14 {
            return self.buffer_format(memory, audio, pointer, args[1]);
        }
        let ObjectKind32::Buffer(buffer) = &self.object(pointer)?.kind else {
            return Err(HResult::INVALID_PARAM);
        };
        match slot {
            3 => {
                if words::<1>(memory, args[1])?[0] != 20 {
                    return Err(HResult::INVALID_PARAM);
                }
                write_words(
                    memory,
                    args[1],
                    &[
                        20,
                        (buffer.native.description.flags & !4) | 8,
                        buffer.native.length(audio)?,
                        0,
                        0,
                    ],
                )?;
            }
            4 => {
                let (play, write) = buffer.native.positions(audio)?;
                let values = [play.to_le_bytes(), write.to_le_bytes()];
                let mut spans = [(GuestAddress(0), &values[0][..]); 2];
                let mut count = 0;
                for (index, address) in [args[1], args[2]].into_iter().enumerate() {
                    if address != 0 {
                        output(memory, address, 4)?;
                        spans[count] = (GuestAddress(address), &values[index]);
                        count += 1;
                    }
                }
                if count == 0 {
                    return Err(HResult::INVALID_PARAM);
                }
                memory.write_spans(&spans[..count])?;
            }
            5 => return get_format(memory, buffer.native.format, args[1], args[2], args[3]),
            6..=8 => {
                let (flag, value) = match slot {
                    6 => (BufferDescription::VOLUME, buffer.native.volume as u32),
                    7 => (BufferDescription::PAN, buffer.native.pan as u32),
                    _ => (BufferDescription::FREQUENCY, buffer.native.frequency),
                };
                if buffer.native.description.flags & flag == 0 {
                    return Err(HResult::CONTROL_UNAVAILABLE);
                }
                write_words(memory, args[1], &[value])?;
            }
            9 => write_words(memory, args[1], &[buffer.native.status(audio)?])?,
            10 => return Err(HResult::ALREADY_INITIALIZED),
            12 => {
                let native = buffer.native;
                native.validate_play(args[1], args[2], args[3])?;
                if native.distance_stops_playback(&self.device(buffer.device)?.native) {
                    let playing = audio.voice_state(native.voice)?.playing;
                    if playing {
                        audio.can_stop_stream(native.voice)?;
                    }
                    let signals = self.stop_signals(pointer)?;
                    let prepared = if signals.is_empty() {
                        None
                    } else {
                        Some(
                            sync.as_deref()
                                .ok_or(HResult::INVALID_CALL)?
                                .prepare_signal_occurrences(signals)
                                .map_err(|_| HResult::INVALID_CALL)?,
                        )
                    };
                    if playing {
                        audio
                            .stop_stream(native.voice)
                            .expect("distance stop was preflighted");
                    }
                    if let Some(prepared) = prepared {
                        sync.unwrap().commit_signals(prepared);
                    }
                } else {
                    native.play(audio, args[1], args[2], args[3])?;
                }
            }
            13 => {
                if buffer.native.primary {
                    return Err(HResult::INVALID_CALL);
                }
                let length = buffer.native.length(audio)?;
                if args[1] >= length {
                    return Err(HResult::INVALID_PARAM);
                }
                audio.seek_stream(
                    buffer.native.voice,
                    args[1] / buffer.native.format.pcm.frame_bytes(),
                )?;
            }
            18 => {
                let voice = buffer.native.voice;
                let playing = audio.voice_state(voice)?.playing;
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
                audio.stop_stream(voice)?;
                if let Some(prepared) = prepared {
                    sync.unwrap().commit_signals(prepared);
                }
            }
            20 => {
                if self.playback_lost {
                    return Err(HResult::OTHER_APP_PRIORITY);
                }
                let index = self.slot(pointer)?;
                let ObjectKind32::Buffer(buffer) = &mut self.records[index].as_mut().unwrap().kind
                else {
                    unreachable!()
                };
                buffer.native.lost = false;
            }
            21 => {
                if buffer.native.description.flags & BufferDescription::EFFECTS == 0 {
                    return Err(HResult::CONTROL_UNAVAILABLE);
                }
                return Err(HResult::FX_UNAVAILABLE);
            }
            22 => {
                if buffer.native.description.flags & BufferDescription::DEFERRED_LOCATION == 0 {
                    return Err(HResult::INVALID_CALL);
                }
                if args[1] & !6 != 0 || args[1] & 2 != 0 {
                    return Err(HResult::CONTROL_UNAVAILABLE);
                }
                if args[2] != 0 {
                    return Err(HResult::INVALID_PARAM);
                }
            }
            23 => return Err(HResult::OBJECT_NOT_FOUND),
            _ => return Err(HResult::UNSUPPORTED),
        }
        Ok(0)
    }
}
