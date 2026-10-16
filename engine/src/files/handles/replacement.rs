use super::*;
use crate::files::tree::OwnedPage32;

impl Vfs32 {
    pub(crate) fn replace_provider_file(
        &mut self,
        path: &[u16],
        bytes: &[u8],
        token: &crate::windows::security::Token32,
    ) -> Result<usize, FileError32> {
        if bytes.len() > self.limits.max_io_bytes {
            return Err(FileError32::Capacity);
        }
        let prepared = self.prepare_open_with_security(
            path,
            FileOpenOptions32 {
                access: FileAccess32::WRITE,
                share: FileShare32::READ,
                disposition: FileDisposition32::CreateAlways,
                ..FileOpenOptions32::default()
            },
            0x4000_0000,
            token,
            None,
        )?;
        self.revision.checked_add(2).ok_or(FileError32::Exhausted)?;
        let old_pages = if prepared.truncate {
            self.node(prepared.result.file)?.content.pages.len()
        } else {
            0
        };
        let count = bytes.len().div_ceil(FILE_PAGE_BYTES);
        if self
            .overlay_pages
            .checked_sub(old_pages)
            .and_then(|used| used.checked_add(count))
            .ok_or(FileError32::DiskFull)?
            > self.limits.overlay_pages
        {
            return Err(FileError32::DiskFull);
        }
        let mut pages = Vec::new();
        pages
            .try_reserve_exact(count)
            .map_err(|_| FileError32::Allocation)?;
        for (index, input) in bytes.chunks(FILE_PAGE_BYTES).enumerate() {
            let mut page = Vec::new();
            page.try_reserve_exact(FILE_PAGE_BYTES)
                .map_err(|_| FileError32::Allocation)?;
            page.resize(FILE_PAGE_BYTES, 0);
            page[..input.len()].copy_from_slice(input);
            pages.push(OwnedPage32 {
                offset: (index * FILE_PAGE_BYTES) as u64,
                bytes: page.into_boxed_slice(),
            });
        }
        prepared.validate(self)?;
        let opened = self.commit_open(prepared);
        let now = self.now;
        let node = self.node_mut(opened.file).unwrap();
        node.content = Content32 {
            asset: None,
            visible: 0,
            pages,
            allocation: 0,
        };
        node.size = bytes.len() as u64;
        node.dirty = true;
        node.attributes = (node.attributes & !ATTR_NORMAL) | ATTR_ARCHIVE;
        node.times.written = now;
        node.changed = now;
        self.overlay_pages += count;
        self.cache.invalidate(opened.file);
        self.pending_writes
            .retain(|stage| stage.file != opened.file);
        self.close_open(opened.open)
            .expect("temporary replacement open and close were preflighted together");
        Ok(bytes.len())
    }
}
