mod calls;

use crate::files::{FileError32, OpenFileId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum KernelFileObject32 {
    File(OpenFileId),
    Mapping(u64),
    CompletionPort(crate::windows::files::completion::CompletionPortId32),
    Null(crate::windows::files::devices::DeviceDescriptionId32),
    Find(u64),
}
#[derive(Clone, Copy)]
struct Entry32 {
    value: u32,
    object: KernelFileObject32,
    access: u32,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "inheritance metadata is retained while child process creation is unsupported"
        )
    )]
    inherit: bool,
}

pub(super) struct KernelFileHandles32 {
    entries: Vec<Entry32>,
    limit: usize,
    next: u32,
}
pub(super) struct PreparedKernelHandle32 {
    value: u32,
    access: u32,
    inherit: bool,
}

impl PreparedKernelHandle32 {
    pub(super) fn handle(&self) -> u32 {
        self.value
    }
}
impl KernelFileHandles32 {
    pub(super) fn commit_process_exit(&mut self) {
        self.entries.clear();
    }
    pub(super) fn new(max_handles: u32) -> Result<Self, FileError32> {
        if max_handles == 0 || max_handles > 65_536 {
            return Err(FileError32::InvalidLimits);
        }
        Ok(Self {
            entries: Vec::new(),
            limit: max_handles as usize,
            next: 0x8000_0000,
        })
    }
    pub(super) fn ensure_capacity(&mut self, additional: usize) -> Result<(), FileError32> {
        if self
            .entries
            .len()
            .checked_add(additional)
            .ok_or(FileError32::Capacity)?
            > self.limit
        {
            return Err(FileError32::Capacity);
        }
        self.next
            .checked_add(
                u32::try_from(additional)
                    .map_err(|_| FileError32::Capacity)?
                    .checked_mul(4)
                    .ok_or(FileError32::Exhausted)?,
            )
            .ok_or(FileError32::Exhausted)?;
        self.entries
            .try_reserve(additional)
            .map_err(|_| FileError32::Allocation)
    }
    pub(super) fn prepare(
        &mut self,
        access: u32,
        inherit: bool,
    ) -> Result<PreparedKernelHandle32, FileError32> {
        if self.entries.len() >= self.limit {
            return Err(FileError32::Capacity);
        }
        self.next.checked_add(4).ok_or(FileError32::Exhausted)?;
        self.entries
            .try_reserve(1)
            .map_err(|_| FileError32::Allocation)?;
        Ok(PreparedKernelHandle32 {
            value: self.next,
            access,
            inherit,
        })
    }
    pub(super) fn commit(
        &mut self,
        prepared: PreparedKernelHandle32,
        object: KernelFileObject32,
    ) -> u32 {
        assert_eq!(prepared.value, self.next);
        self.next += 4;
        self.entries.push(Entry32 {
            value: prepared.value,
            object,
            access: prepared.access,
            inherit: prepared.inherit,
        });
        prepared.value
    }
    pub(super) fn object(&self, handle: u32) -> Result<KernelFileObject32, FileError32> {
        Ok(self.entry(handle)?.object)
    }
    pub(super) fn file(&self, handle: u32) -> Result<OpenFileId, FileError32> {
        match self.object(handle)? {
            KernelFileObject32::File(open) => Ok(open),
            _ => Err(FileError32::InvalidHandle),
        }
    }
    pub(super) fn io_target(
        &self,
        handle: u32,
    ) -> Result<crate::windows::files::completion::CompletionTarget32, FileError32> {
        match self.object(handle)? {
            KernelFileObject32::File(open) => Ok(
                crate::windows::files::completion::CompletionTarget32::File(open),
            ),
            KernelFileObject32::Null(id) => Ok(
                crate::windows::files::completion::CompletionTarget32::Null(id),
            ),
            _ => Err(FileError32::InvalidHandle),
        }
    }
    pub(super) fn io_count(
        &self,
        target: crate::windows::files::completion::CompletionTarget32,
    ) -> usize {
        let object = match target {
            crate::windows::files::completion::CompletionTarget32::Null(id) => {
                KernelFileObject32::Null(id)
            }
            crate::windows::files::completion::CompletionTarget32::File(open) => {
                KernelFileObject32::File(open)
            }
        };
        self.entries
            .iter()
            .filter(|entry| entry.object == object)
            .count()
    }
    pub(super) fn file_count(&self, open: OpenFileId) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.object == KernelFileObject32::File(open))
            .count()
    }
    pub(super) fn access(&self, handle: u32) -> Result<u32, FileError32> {
        Ok(self.entry(handle)?.access)
    }
    #[cfg(test)]
    pub(super) fn inherit(&self, handle: u32) -> Result<bool, FileError32> {
        Ok(self.entry(handle)?.inherit)
    }
    pub(super) fn remove(&mut self, handle: u32) -> Result<KernelFileObject32, FileError32> {
        let index = self.index(handle)?;
        Ok(self.entries.remove(index).object)
    }
    fn index(&self, handle: u32) -> Result<usize, FileError32> {
        self.entries
            .binary_search_by_key(&handle, |entry| entry.value)
            .map_err(|_| FileError32::InvalidHandle)
    }
    fn entry(&self, handle: u32) -> Result<&Entry32, FileError32> {
        Ok(&self.entries[self.index(handle)?])
    }
}

#[cfg(test)]
mod tests;
