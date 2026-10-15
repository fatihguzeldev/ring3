use super::{context::*, views::memory_error};
use crate::{
    files::{FileError32, FileId, FileIo32, FileReadRequest32},
    memory::{AddressSpace, PAGE_SIZE},
    process::windows::files::FileRuntime32,
};

struct WritebackPage32 {
    group: usize,
    page: usize,
    offset: u64,
    length: usize,
    version: u64,
}
pub(super) struct MappingWriteback32 {
    file: Option<FileId>,
    pages: Vec<WritebackPage32>,
    chunks: Vec<usize>,
}
impl MappingWriteback32 {
    pub(super) fn mutations(&self) -> u64 {
        self.chunks.len() as u64
    }
}
impl FileRuntime32 {
    pub(super) fn prepare_mapping_writeback(
        &self,
        memory: &AddressSpace,
        key: MappingBacking32,
        range: Option<(u64, u64)>,
    ) -> Result<MappingWriteback32, FileError32> {
        let MappingBacking32::File(file) = key else {
            return Ok(MappingWriteback32 {
                file: None,
                pages: Vec::new(),
                chunks: Vec::new(),
            });
        };
        let file_size = self.vfs.info(file)?.size;
        let mut selected = Vec::new();
        for (group, pages) in self
            .mappings
            .pages
            .iter()
            .enumerate()
            .filter(|(_, pages)| pages.key == key)
        {
            let (first, last) = range.map_or((0, pages.offsets.len()), |(start, end)| {
                (
                    pages.offsets.partition_point(|offset| {
                        offset.saturating_add(u64::from(PAGE_SIZE)) <= start
                    }),
                    pages.offsets.partition_point(|offset| *offset < end),
                )
            });
            for (within, &offset) in pages.offsets[first..last].iter().enumerate() {
                let page = first + within;
                let version = memory
                    .shared_content_version(&pages.backing, page)
                    .map_err(memory_error)?;
                if version == pages.flushed[page] || offset >= file_size {
                    continue;
                }
                let length = (file_size - offset).min(u64::from(PAGE_SIZE)) as usize;
                if length > self.vfs.max_io_bytes() {
                    return Err(FileError32::Capacity);
                }
                selected
                    .try_reserve(1)
                    .map_err(|_| FileError32::Allocation)?;
                selected.push(WritebackPage32 {
                    group,
                    page,
                    offset,
                    length,
                    version,
                });
            }
        }
        selected.sort_unstable_by_key(|page| page.offset);
        let mut chunks = Vec::new();
        let mut bytes = 0;
        for (index, page) in selected.iter().enumerate() {
            if bytes + page.length > self.vfs.max_io_bytes() {
                chunks.try_reserve(1).map_err(|_| FileError32::Allocation)?;
                chunks.push(index);
                bytes = 0;
            }
            bytes += page.length;
        }
        if !selected.is_empty() {
            chunks.try_reserve(1).map_err(|_| FileError32::Allocation)?;
            chunks.push(selected.len());
        }
        Ok(MappingWriteback32 {
            file: Some(file),
            pages: selected,
            chunks,
        })
    }
    pub(super) fn flush_mapping_pages(
        &mut self,
        memory: &AddressSpace,
        key: MappingBacking32,
        range: Option<(u64, u64)>,
    ) -> Result<Option<FileReadRequest32>, FileError32> {
        let prepared = self.prepare_mapping_writeback(memory, key, range)?;
        self.flush_mapping_writeback(memory, prepared)
    }
    pub(super) fn flush_mapping_writeback(
        &mut self,
        memory: &AddressSpace,
        prepared: MappingWriteback32,
    ) -> Result<Option<FileReadRequest32>, FileError32> {
        let Some(file) = prepared.file else {
            return Ok(None);
        };
        let mut first = 0;
        for last in prepared.chunks {
            let pages = &prepared.pages[first..last];
            let length = pages.iter().map(|page| page.length).sum();
            let mut data = Vec::new();
            data.try_reserve_exact(length)
                .map_err(|_| FileError32::Allocation)?;
            data.resize(length, 0);
            let mut at = 0;
            for page in pages {
                memory
                    .read_shared_pages(
                        &self.mappings.pages[page.group].backing,
                        page.page * PAGE_SIZE as usize,
                        &mut data[at..at + page.length],
                    )
                    .map_err(memory_error)?;
                at += page.length;
            }
            let mut spans = Vec::new();
            spans
                .try_reserve_exact(pages.len())
                .map_err(|_| FileError32::Allocation)?;
            let mut at = 0;
            for page in pages {
                spans.push((page.offset, &data[at..at + page.length]));
                at += page.length;
            }
            match self.vfs.prepare_writes_at(file, &spans)? {
                FileIo32::NeedBytes(request) => return Ok(Some(request)),
                FileIo32::Ready(write) => {
                    write.validate(&self.vfs)?;
                    self.vfs.commit_write(write);
                    for page in pages {
                        self.mappings.pages[page.group].flushed[page.page] = page.version;
                    }
                }
            }
            first = last;
        }
        Ok(None)
    }
}
