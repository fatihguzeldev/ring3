use super::User32Error32 as Error;
use crate::windows::window::WindowHandle32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ClipboardOwner32 {
    pub thread: u32,
    pub window: Option<WindowHandle32>,
}

pub(crate) use crate::windows::memory::GlobalMemory32 as ClipboardMemory32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClipboardContent32 {
    Memory(ClipboardMemory32),
    Delayed,
    Synthesized {
        source: u32,
        memory: Option<ClipboardMemory32>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ClipboardEntry32 {
    pub format: u32,
    pub content: ClipboardContent32,
}

struct RegisteredFormat32 {
    name: Vec<u16>,
    key: Vec<u16>,
}

pub(crate) struct ClipboardState32 {
    open: Option<ClipboardOwner32>,
    owner: Option<ClipboardOwner32>,
    entries: Vec<ClipboardEntry32>,
    names: Vec<RegisteredFormat32>,
    sequence: u32,
    locale: u32,
}

impl Default for ClipboardState32 {
    fn default() -> Self {
        Self {
            open: None,
            owner: None,
            entries: Vec::new(),
            names: Vec::new(),
            sequence: 1,
            locale: 0,
        }
    }
}

impl ClipboardState32 {
    pub(crate) fn open(&mut self, caller: ClipboardOwner32) -> Result<(), Error> {
        if self.open.is_some_and(|open| open != caller) {
            return Err(Error::Win32(5));
        }
        self.open = Some(caller);
        Ok(())
    }

    pub(crate) fn check_open(&self, thread: u32) -> Result<ClipboardOwner32, Error> {
        self.open
            .filter(|open| open.thread == thread)
            .ok_or(Error::Win32(1418))
    }

    pub(crate) fn close(&mut self, thread: u32, locale: u32) -> Result<(), Error> {
        self.check_open(thread)?;
        let source = self
            .entries
            .iter()
            .find(|entry| matches!(entry.format, 1 | 7 | 13))
            .map(|entry| entry.format);
        if let Some(source) = source {
            let missing = [16, 1, 7, 13].map(|format| !self.available(format));
            self.entries
                .try_reserve(missing.iter().filter(|&&missing| missing).count())
                .map_err(|_| Error::Allocation)?;
            if missing[0] {
                self.locale = locale;
            }
            for (format, missing) in [16, 1, 7, 13].into_iter().zip(missing) {
                if missing {
                    self.entries.push(ClipboardEntry32 {
                        format,
                        content: ClipboardContent32::Synthesized {
                            source,
                            memory: None,
                        },
                    });
                }
            }
        }
        self.open = None;
        Ok(())
    }

    pub(crate) fn entries(&self) -> &[ClipboardEntry32] {
        &self.entries
    }

    pub(crate) fn owner(&self) -> Option<ClipboardOwner32> {
        self.owner
    }

    pub(crate) fn opened_by(&self) -> Option<ClipboardOwner32> {
        self.open
    }

    pub(crate) fn locale(&self) -> u32 {
        self.locale
    }

    pub(crate) fn sequence(&self) -> u32 {
        self.sequence
    }

    pub(crate) fn available(&self, format: u32) -> bool {
        self.entries.iter().any(|entry| entry.format == format)
    }

    pub(crate) fn get(&self, format: u32) -> Option<ClipboardContent32> {
        self.entries
            .iter()
            .find(|entry| entry.format == format)
            .map(|entry| entry.content)
    }

    pub(crate) fn prepare_set(&mut self, format: u32) -> Result<(), Error> {
        if format == 0 {
            return Err(Error::Win32(1418));
        }
        if format > 0xffff {
            return Err(Error::Win32(87));
        }
        if !self.available(format) {
            if self.entries.len() == 65535 {
                return Err(Error::Capacity);
            }
            self.entries.try_reserve(1).map_err(|_| Error::Allocation)?;
        }
        Ok(())
    }

    pub(crate) fn replaced_memory(
        &self,
        format: u32,
    ) -> impl Iterator<Item = ClipboardMemory32> + '_ {
        self.entries
            .iter()
            .filter(move |entry| entry.replaced_by(format))
            .filter_map(|entry| entry.content.memory())
    }

    pub(crate) fn commit_set(&mut self, format: u32, content: ClipboardContent32) {
        self.entries
            .retain(|entry| entry.format == format || !entry.replaced_by(format));
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.format == format) {
            entry.content = content;
        } else {
            self.entries.push(ClipboardEntry32 { format, content });
        }
        self.sequence = self.sequence.wrapping_add(1);
    }

    pub(crate) fn commit_synthesized(&mut self, format: u32, owned: ClipboardMemory32) {
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.format == format)
            .expect("synthesized format was preflighted");
        let ClipboardContent32::Synthesized { memory, .. } = &mut entry.content else {
            panic!("derived format retains source identity")
        };
        *memory = Some(owned);
    }

    pub(crate) fn commit_empty(&mut self, owner: ClipboardOwner32, locale: u32) {
        self.locale = locale;
        self.entries.clear();
        self.owner = Some(owner);
        self.sequence = self.sequence.wrapping_add(1);
    }

    pub(crate) fn next(&self, thread: u32, previous: u32) -> Result<Option<u32>, Error> {
        self.check_open(thread)?;
        if self.entries.is_empty() {
            return Ok(None);
        }
        let index = if previous == 0 {
            0
        } else {
            self.entries
                .iter()
                .position(|entry| entry.format == previous)
                .ok_or(Error::Win32(87))?
                + 1
        };
        Ok(self.entries.get(index).map(|entry| entry.format))
    }

    pub(crate) fn priority(&self, requested: &[u32]) -> i32 {
        if self.entries.is_empty() {
            0
        } else {
            requested
                .iter()
                .copied()
                .find(|&format| self.available(format))
                .map_or(-1, |format| format as i32)
        }
    }

    pub(crate) fn register(&mut self, name: &[u16]) -> Result<u32, Error> {
        if name.is_empty() || name.len() > 255 {
            return Err(Error::Win32(87));
        }
        let key = crate::windows::text::ordinal_key(name);
        if let Some(index) = self.names.iter().position(|entry| entry.key == key) {
            return Ok(0xc000 + index as u32);
        }
        if self.names.len() == 16384 {
            return Err(Error::Capacity);
        }
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(name.len())
            .map_err(|_| Error::Allocation)?;
        owned.extend_from_slice(name);
        self.names.try_reserve(1).map_err(|_| Error::Allocation)?;
        let format = 0xc000 + self.names.len() as u32;
        self.names.push(RegisteredFormat32 { name: owned, key });
        Ok(format)
    }

    pub(crate) fn registered_name(&self, format: u32) -> Option<&[u16]> {
        self.names
            .get(format.checked_sub(0xc000)? as usize)
            .map(|entry| entry.name.as_slice())
    }

    pub(crate) fn retire_window(&mut self, window: WindowHandle32) {
        if self.open.is_some_and(|open| open.window == Some(window)) {
            self.open.as_mut().unwrap().window = None;
        }
        if self.owner.is_some_and(|owner| owner.window == Some(window)) {
            self.retire_owner();
        }
    }

    pub(crate) fn retire_thread(&mut self, thread: u32) {
        if self.open.is_some_and(|open| open.thread == thread) {
            self.open = None;
        }
        if self.owner.is_some_and(|owner| owner.thread == thread) {
            self.retire_owner();
        }
    }

    fn retire_owner(&mut self) {
        self.owner = None;
        let delayed =
            [1, 7, 13].map(|format| self.get(format) == Some(ClipboardContent32::Delayed));
        self.entries
            .retain(|entry| entry.content != ClipboardContent32::Delayed);
        self.entries.retain(|entry| match entry.content {
            ClipboardContent32::Synthesized {
                source,
                memory: None,
            } => ![1, 7, 13]
                .into_iter()
                .zip(delayed)
                .any(|(format, unavailable)| format == source && unavailable),
            _ => true,
        });
    }
}

impl ClipboardEntry32 {
    pub(crate) fn replaced_by(self, format: u32) -> bool {
        self.format == format
            || matches!(self.content, ClipboardContent32::Synthesized { source, .. } if source == format || format == 16 && self.format != 16)
    }
}

impl ClipboardContent32 {
    pub(crate) fn memory(self) -> Option<ClipboardMemory32> {
        match self {
            Self::Memory(memory)
            | Self::Synthesized {
                memory: Some(memory),
                ..
            } => Some(memory),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
