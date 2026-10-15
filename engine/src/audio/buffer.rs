use super::{AudioBufferId32, AudioContext, AudioError32, PcmFormat32, SampleEncoding32};

#[derive(Default)]
pub(super) struct BufferSlot {
    pub generation: u32,
    pub buffer: Option<PcmBuffer>,
}

pub(super) struct PcmBuffer {
    pub format: PcmFormat32,
    pub bytes: Vec<u8>,
    pub encoding: SampleEncoding32,
}

impl PcmBuffer {
    pub fn frames(&self) -> u32 {
        self.bytes.len() as u32 / self.format.frame_bytes()
    }

    pub fn sample(&self, frame: u32, channel: usize) -> f32 {
        let channel = if self.format.channels == 1 {
            0
        } else {
            channel
        };
        let at = frame as usize * self.format.frame_bytes() as usize
            + channel * usize::from(self.format.bits_per_sample / 8);
        match (self.encoding, self.format.bits_per_sample) {
            (SampleEncoding32::Integer, 8) => (f32::from(self.bytes[at]) - 128.0) / 128.0,
            (SampleEncoding32::Integer, 16) => {
                f32::from(i16::from_le_bytes([self.bytes[at], self.bytes[at + 1]])) / 32768.0
            }
            (SampleEncoding32::Integer, 24) => {
                let sample =
                    i32::from_le_bytes([0, self.bytes[at], self.bytes[at + 1], self.bytes[at + 2]]);
                sample as f32 / 2_147_483_648.0
            }
            (SampleEncoding32::Integer, 32) => {
                i32::from_le_bytes(self.bytes[at..at + 4].try_into().unwrap()) as f32
                    / 2_147_483_648.0
            }
            (SampleEncoding32::Float, 32) => {
                let value = f32::from_le_bytes(self.bytes[at..at + 4].try_into().unwrap());
                if value.is_finite() { value } else { 0.0 }
            }
            _ => unreachable!(),
        }
    }
}

impl AudioContext {
    pub fn create_buffer(
        &mut self,
        format: PcmFormat32,
        byte_length: u32,
    ) -> Result<AudioBufferId32, AudioError32> {
        self.ready()?;
        if format.sample_rate != self.config.sample_rate
            || !(1..=2).contains(&format.channels)
            || !matches!(format.bits_per_sample, 8 | 16)
        {
            return Err(AudioError32::UnsupportedFormat);
        }
        self.create_pcm_buffer(format, SampleEncoding32::Integer, byte_length)
    }

    pub fn create_stream_buffer(
        &mut self,
        format: PcmFormat32,
        encoding: SampleEncoding32,
        byte_length: u32,
    ) -> Result<AudioBufferId32, AudioError32> {
        if !(100..=200_000).contains(&format.sample_rate)
            || !(1..=2).contains(&format.channels)
            || !matches!(
                (encoding, format.bits_per_sample),
                (SampleEncoding32::Integer, 8 | 16 | 24 | 32) | (SampleEncoding32::Float, 32)
            )
        {
            return Err(AudioError32::UnsupportedFormat);
        }
        self.create_pcm_buffer(format, encoding, byte_length)
    }

    fn create_pcm_buffer(
        &mut self,
        format: PcmFormat32,
        encoding: SampleEncoding32,
        byte_length: u32,
    ) -> Result<AudioBufferId32, AudioError32> {
        if byte_length == 0 || !byte_length.is_multiple_of(format.frame_bytes()) {
            return Err(AudioError32::InvalidRequest);
        }
        let total = self
            .buffer_bytes
            .checked_add(byte_length)
            .filter(|total| *total <= self.config.max_buffer_bytes)
            .ok_or(AudioError32::Capacity)?;
        let (index, generation) = self
            .buffers
            .iter()
            .enumerate()
            .find_map(|(index, slot)| {
                slot.buffer
                    .is_none()
                    .then(|| slot.generation.checked_add(1).map(|next| (index, next)))
                    .flatten()
            })
            .ok_or(AudioError32::Capacity)?;
        let silence = if format.bits_per_sample == 8 { 128 } else { 0 };
        let bytes = super::allocated(byte_length as usize, || silence)?;
        self.buffers[index].buffer = Some(PcmBuffer {
            format,
            bytes,
            encoding,
        });
        self.buffers[index].generation = generation;
        self.buffer_bytes = total;
        Ok(AudioBufferId32 {
            epoch: self.epoch,
            slot: index as u32,
            generation,
        })
    }

    /// publishes complete PCM frames; a wrapped write commits both spans together.
    pub fn write_buffer(
        &mut self,
        id: AudioBufferId32,
        byte_offset: u32,
        bytes: &[u8],
    ) -> Result<(), AudioError32> {
        self.ready()?;
        let buffer = self.buffer(id)?;
        let frame_bytes = buffer.format.frame_bytes() as usize;
        if byte_offset as usize >= buffer.bytes.len()
            || !byte_offset.is_multiple_of(frame_bytes as u32)
            || bytes.is_empty()
            || bytes.len() > buffer.bytes.len()
            || !bytes.len().is_multiple_of(frame_bytes)
        {
            return Err(AudioError32::InvalidRequest);
        }
        let buffer = self.buffers[id.slot as usize].buffer.as_mut().unwrap();
        let at = byte_offset as usize;
        let first = bytes.len().min(buffer.bytes.len() - at);
        buffer.bytes[at..at + first].copy_from_slice(&bytes[..first]);
        buffer.bytes[..bytes.len() - first].copy_from_slice(&bytes[first..]);
        Ok(())
    }

    pub fn release_buffer(&mut self, id: AudioBufferId32) -> Result<(), AudioError32> {
        self.ready()?;
        self.release_stream_buffer(id)
    }

    pub fn release_stream_buffer(&mut self, id: AudioBufferId32) -> Result<(), AudioError32> {
        let length = self.buffer(id)?.bytes.len() as u32;
        if self
            .voices
            .iter()
            .any(|slot| slot.voice.as_ref().is_some_and(|voice| voice.buffer == id))
        {
            return Err(AudioError32::BufferInUse);
        }
        self.buffers[id.slot as usize].buffer = None;
        self.buffer_bytes -= length;
        Ok(())
    }

    pub fn stream_format(
        &self,
        id: AudioBufferId32,
    ) -> Result<(PcmFormat32, SampleEncoding32), AudioError32> {
        let buffer = self.buffer(id)?;
        Ok((buffer.format, buffer.encoding))
    }

    pub fn stream_bytes(&self, id: AudioBufferId32) -> Result<&[u8], AudioError32> {
        Ok(&self.buffer(id)?.bytes)
    }

    pub fn write_stream_buffer(
        &mut self,
        id: AudioBufferId32,
        offset: u32,
        first: &[u8],
        second: &[u8],
    ) -> Result<(), AudioError32> {
        let buffer = self.buffer(id)?;
        let offset = offset as usize;
        if offset >= buffer.bytes.len()
            || first.len() > buffer.bytes.len() - offset
            || second.len() > offset
        {
            return Err(AudioError32::InvalidRequest);
        }
        let buffer = self.buffers[id.slot as usize].buffer.as_mut().unwrap();
        buffer.bytes[offset..offset + first.len()].copy_from_slice(first);
        buffer.bytes[..second.len()].copy_from_slice(second);
        Ok(())
    }

    pub(super) fn buffer(&self, id: AudioBufferId32) -> Result<&PcmBuffer, AudioError32> {
        if id.epoch != self.epoch || id.generation == 0 {
            return Err(AudioError32::InvalidBuffer);
        }
        self.buffers
            .get(id.slot as usize)
            .filter(|slot| slot.generation == id.generation)
            .and_then(|slot| slot.buffer.as_ref())
            .ok_or(AudioError32::InvalidBuffer)
    }
}
