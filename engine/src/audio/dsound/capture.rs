use super::{HResult, LockSpan, WaveFormat};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureRequest {
    pub epoch: u64,
    pub object: u64,
    pub revision: u64,
    pub first_frame: u64,
    pub format: WaveFormat,
    pub maximum_frames: u32,
    pub looping: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureProgress {
    pub first_byte: u32,
    pub last_byte: u32,
    pub wrapped: bool,
    pub stopped: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedCaptureWrite {
    revision: u64,
    first_frame: u64,
    frames: u32,
    bytes: u32,
}

pub struct CaptureRing {
    pub format: WaveFormat,
    pub flags: u32,
    bytes: Vec<u8>,
    revision: u64,
    frames: u64,
    active: bool,
    looping: bool,
}
impl CaptureRing {
    pub fn new(format: WaveFormat, length: u32, flags: u32, budget: u32) -> Result<Self, HResult> {
        let format = format.validate()?;
        if flags & !0x8000_0200 != 0
            || length < 4
            || !length.is_multiple_of(format.pcm.frame_bytes())
        {
            return Err(HResult::INVALID_PARAM);
        }
        if length > budget {
            return Err(HResult::OUT_OF_MEMORY);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length as usize)
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        bytes.resize(
            length as usize,
            if format.pcm.bits_per_sample == 8 {
                128
            } else {
                0
            },
        );
        Ok(Self {
            format,
            flags,
            bytes,
            revision: 1,
            frames: 0,
            active: false,
            looping: false,
        })
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn status(&self) -> u32 {
        u32::from(self.active) | if self.active && self.looping { 2 } else { 0 }
    }
    pub fn position(&self) -> u32 {
        ((self.frames % (self.bytes.len() as u64 / u64::from(self.format.pcm.frame_bytes())))
            * u64::from(self.format.pcm.frame_bytes())) as u32
    }
    pub fn start(&mut self, flags: u32) -> Result<(), HResult> {
        if flags & !1 != 0 {
            return Err(HResult::INVALID_PARAM);
        }
        if self.active {
            return Ok(());
        }
        self.revision = self.revision.checked_add(1).ok_or(HResult::INVALID_CALL)?;
        self.active = true;
        self.looping = flags & 1 != 0;
        self.frames = 0;
        Ok(())
    }
    pub fn stop(&mut self) -> Result<bool, HResult> {
        if !self.active {
            return Ok(false);
        }
        let revision = self.revision.checked_add(1).ok_or(HResult::INVALID_CALL)?;
        self.revision = revision;
        self.active = false;
        Ok(true)
    }
    pub fn request(&self, epoch: u64, object: u64) -> Option<CaptureRequest> {
        if !self.active {
            return None;
        }
        let capacity = self.bytes.len() as u32 / self.format.pcm.frame_bytes();
        Some(CaptureRequest {
            epoch,
            object,
            revision: self.revision,
            first_frame: self.frames,
            format: self.format,
            maximum_frames: if self.looping {
                capacity
            } else {
                capacity - self.frames as u32
            },
            looping: self.looping,
        })
    }
    pub fn prepare_write(
        &self,
        request: CaptureRequest,
        epoch: u64,
        object: u64,
        bytes: &[u8],
    ) -> Result<PreparedCaptureWrite, HResult> {
        if self.request(epoch, object) != Some(request)
            || bytes.is_empty()
            || !bytes
                .len()
                .is_multiple_of(self.format.pcm.frame_bytes() as usize)
            || bytes.len()
                > request.maximum_frames as usize * self.format.pcm.frame_bytes() as usize
        {
            return Err(HResult::INVALID_CALL);
        }
        let frames = bytes.len() as u32 / self.format.pcm.frame_bytes();
        self.frames
            .checked_add(u64::from(frames))
            .ok_or(HResult::INVALID_CALL)?;
        Ok(PreparedCaptureWrite {
            revision: self.revision,
            first_frame: self.frames,
            frames,
            bytes: bytes.len() as u32,
        })
    }
    pub fn progress(&self, prepared: PreparedCaptureWrite) -> CaptureProgress {
        let first = self.position();
        let end = u64::from(first) + u64::from(prepared.bytes);
        let length = self.bytes.len() as u64;
        CaptureProgress {
            first_byte: first,
            last_byte: (end % length) as u32,
            wrapped: end >= length,
            stopped: !self.looping
                && (prepared.first_frame + u64::from(prepared.frames))
                    * u64::from(self.format.pcm.frame_bytes())
                    == length,
        }
    }
    pub fn commit_write(&mut self, prepared: PreparedCaptureWrite, bytes: &[u8]) {
        assert_eq!(
            (self.revision, self.frames, bytes.len()),
            (
                prepared.revision,
                prepared.first_frame,
                prepared.bytes as usize
            )
        );
        let offset = self.position() as usize;
        let first = bytes.len().min(self.bytes.len() - offset);
        self.bytes[offset..offset + first].copy_from_slice(&bytes[..first]);
        self.bytes[..bytes.len() - first].copy_from_slice(&bytes[first..]);
        self.frames += u64::from(prepared.frames);
        if !self.looping
            && self.frames * u64::from(self.format.pcm.frame_bytes()) == self.bytes.len() as u64
        {
            self.active = false;
        }
    }
    pub fn lock_span(
        &self,
        offset: u32,
        length: u32,
        flags: u32,
        wrap: bool,
    ) -> Result<LockSpan, HResult> {
        if flags & !1 != 0 {
            return Err(HResult::INVALID_PARAM);
        }
        let size = self.bytes.len() as u32;
        let length = if flags & 1 != 0 { size } else { length };
        if offset >= size || length > size {
            return Err(HResult::INVALID_PARAM);
        }
        let first = length.min(size - offset);
        Ok(LockSpan {
            offset,
            first,
            second: if wrap { length - first } else { 0 },
        })
    }
    pub fn write_locked(
        &mut self,
        offset: u32,
        first: &[u8],
        second: &[u8],
    ) -> Result<(), HResult> {
        let offset = offset as usize;
        if offset >= self.bytes.len()
            || first.len() > self.bytes.len() - offset
            || second.len() > offset
        {
            return Err(HResult::INVALID_PARAM);
        }
        self.bytes[offset..offset + first.len()].copy_from_slice(first);
        self.bytes[..second.len()].copy_from_slice(second);
        Ok(())
    }
    pub fn invalidate(&mut self) -> Result<(), HResult> {
        self.can_invalidate()?;
        let revision = self
            .revision
            .checked_add(if self.active { 2 } else { 1 })
            .ok_or(HResult::INVALID_CALL)?;
        self.revision = revision;
        self.active = false;
        Ok(())
    }
    pub fn can_invalidate(&self) -> Result<(), HResult> {
        self.revision
            .checked_add(if self.active { 2 } else { 1 })
            .ok_or(HResult::INVALID_CALL)?;
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn revision_for_test(&mut self, revision: u64) {
        self.revision = revision;
    }
}
