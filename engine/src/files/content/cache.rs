use crate::files::{FileError32, FileId, FileReadRequest32};

pub(in crate::files) struct CachePage32 {
    pub request: FileReadRequest32,
    pub bytes: Box<[u8]>,
    pub coverage: Box<[u8]>,
    pub supplied: usize,
    pub age: u64,
}
pub(in crate::files) struct ReadStage32 {
    pub owner: u64,
    pub file: FileId,
    pub revision: u64,
    pub offset: u64,
    pub length: usize,
    pub bytes: Vec<u8>,
    pub filled: usize,
    pub waiting: Option<FileReadRequest32>,
}
pub(in crate::files) struct ContentCache32 {
    pub pages: Vec<CachePage32>,
    pub stages: Vec<ReadStage32>,
    pub next_request: u64,
    pub age: u64,
}
impl ContentCache32 {
    pub fn new(pages: usize, requests: usize) -> Result<Self, FileError32> {
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(pages)
            .map_err(|_| FileError32::Allocation)?;
        let mut stages = Vec::new();
        stages
            .try_reserve_exact(requests)
            .map_err(|_| FileError32::Allocation)?;
        Ok(Self {
            pages: entries,
            stages,
            next_request: 1,
            age: 1,
        })
    }
    pub fn invalidate(&mut self, file: FileId) {
        self.pages.retain(|p| p.request.file != file);
        self.stages.retain(|p| p.file != file);
    }
    pub fn bytes(&mut self, asset: u64, offset: u64) -> Option<&[u8]> {
        let page = self.pages.iter_mut().find(|p| {
            p.request.asset == asset
                && p.request.offset == offset
                && p.supplied == p.request.length as usize
        })?;
        self.age = self.age.saturating_add(1);
        page.age = self.age;
        Some(&page.bytes[..page.request.length as usize])
    }
    pub fn request(
        &mut self,
        mut request: FileReadRequest32,
        previous: Option<FileReadRequest32>,
        capacity: usize,
        requests: usize,
    ) -> Result<FileReadRequest32, FileError32> {
        if let Some(previous) = previous {
            if self.pages.iter().any(|p| p.request == previous) {
                return Ok(previous);
            }
        }
        let length = request.length;
        if self
            .pages
            .iter()
            .filter(|p| p.supplied < p.request.length as usize)
            .count()
            >= requests
        {
            return Err(FileError32::Capacity);
        }
        let next = self
            .next_request
            .checked_add(1)
            .ok_or(FileError32::Exhausted)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length as usize)
            .map_err(|_| FileError32::Allocation)?;
        bytes.resize(length as usize, 0);
        let mut coverage = Vec::new();
        coverage
            .try_reserve_exact((length as usize).div_ceil(8))
            .map_err(|_| FileError32::Allocation)?;
        coverage.resize((length as usize).div_ceil(8), 0);
        if self.pages.len() >= capacity {
            let index = self
                .pages
                .iter()
                .enumerate()
                .filter(|(_, p)| p.supplied == p.request.length as usize)
                .min_by_key(|(_, p)| p.age)
                .map(|(i, _)| i)
                .ok_or(FileError32::Capacity)?;
            self.pages.swap_remove(index);
        }
        request.request = self.next_request;
        self.next_request = next;
        self.age = self.age.saturating_add(1);
        self.pages.push(CachePage32 {
            request,
            bytes: bytes.into_boxed_slice(),
            coverage: coverage.into_boxed_slice(),
            supplied: 0,
            age: self.age,
        });
        Ok(request)
    }
    pub fn supply(
        &mut self,
        request: FileReadRequest32,
        offset: u32,
        bytes: &[u8],
    ) -> Result<(), FileError32> {
        if bytes.is_empty() || bytes.len() > 4096 {
            return Err(FileError32::InvalidParameter);
        }
        let page = self
            .pages
            .iter_mut()
            .find(|p| p.request == request)
            .ok_or(FileError32::Stale)?;
        let start = offset as usize;
        let end = start
            .checked_add(bytes.len())
            .ok_or(FileError32::InvalidParameter)?;
        if end > page.bytes.len() {
            return Err(FileError32::InvalidParameter);
        }
        for (index, &byte) in bytes.iter().enumerate() {
            let position = start + index;
            if page.coverage[position / 8] & (1 << (position % 8)) != 0
                && page.bytes[position] != byte
            {
                return Err(FileError32::InvalidParameter);
            }
        }
        for (index, &byte) in bytes.iter().enumerate() {
            let position = start + index;
            let bit = 1 << (position % 8);
            if page.coverage[position / 8] & bit == 0 {
                page.coverage[position / 8] |= bit;
                page.supplied += 1;
            }
            page.bytes[position] = byte;
        }
        Ok(())
    }
}
