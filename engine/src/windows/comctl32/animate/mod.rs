mod avi;
use crate::windows::user32::User32Error32 as E;
pub(crate) use avi::AviClip32;

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Animate32 {
    pub clip: Option<AviClip32>,
    pub loading: Vec<u8>,
    pub loading_owner: Option<u64>,
    pub current: usize,
    pub playing: bool,
    pub first: usize,
    pub last: usize,
    pub remaining: u32,
}
impl Animate32 {
    pub fn play_range(&self, first: u16, last: u16) -> Result<(usize, usize), E> {
        let clip = self.clip.as_ref().ok_or(E::Win32(87))?;
        let first = usize::from(first);
        let last = if last == u16::MAX {
            clip.frames.len() - 1
        } else {
            usize::from(last)
        };
        if first > last || last >= clip.frames.len() {
            return Err(E::Win32(87));
        }
        Ok((first, last))
    }
    pub fn advance(&mut self) -> bool {
        if !self.playing {
            return false;
        }
        if self.current < self.last {
            self.current += 1;
            return false;
        }
        if self.remaining == u32::MAX || self.remaining > 1 {
            if self.remaining != u32::MAX {
                self.remaining -= 1;
            }
            self.current = self.first;
            false
        } else {
            self.playing = false;
            true
        }
    }
}
#[cfg(test)]
pub(crate) mod tests;
