mod lookup;
mod mount;

use super::{ATTR_DIRECTORY, AssetRef32, FileError32, FileId, FileInfo32, FileTimes32};

#[derive(Clone, Debug)]
pub(super) struct OwnedPage32 {
    pub offset: u64,
    pub bytes: Box<[u8]>,
}
#[derive(Clone, Debug, Default)]
pub(super) struct Content32 {
    pub asset: Option<AssetRef32>,
    pub visible: u64,
    pub pages: Vec<OwnedPage32>,
    pub allocation: u64,
}
#[derive(Clone, Debug)]
pub(super) struct Node32 {
    pub id: FileId,
    pub attributes: u32,
    pub times: FileTimes32,
    pub changed: u64,
    pub size: u64,
    pub linked: bool,
    pub delete_pending: bool,
    pub delete_requested: bool,
    pub descriptions: u32,
    pub content_revision: u64,
    pub dirty: bool,
    pub content: Content32,
    pub security: crate::windows::security::SecurityDescriptor32,
}
impl Node32 {
    pub fn info(&self) -> FileInfo32 {
        FileInfo32 {
            id: self.id,
            attributes: self.attributes,
            size: self.size,
            times: self.times,
            links: u32::from(self.linked),
            delete_pending: self.delete_pending,
        }
    }
    pub fn directory(&self) -> bool {
        self.attributes & ATTR_DIRECTORY != 0
    }
}
#[derive(Clone, Debug)]
pub(super) struct NamespaceEntry32 {
    pub path: Vec<u16>,
    pub key: Vec<u16>,
    pub file: FileId,
}

impl Content32 {
    pub fn snapshot(&self) -> Result<Self, FileError32> {
        let mut pages = Vec::new();
        pages
            .try_reserve_exact(self.pages.len())
            .map_err(|_| FileError32::Allocation)?;
        for page in &self.pages {
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(page.bytes.len())
                .map_err(|_| FileError32::Allocation)?;
            bytes.extend_from_slice(&page.bytes);
            pages.push(OwnedPage32 {
                offset: page.offset,
                bytes: bytes.into_boxed_slice(),
            });
        }
        Ok(Self {
            asset: self.asset,
            visible: self.visible,
            pages,
            allocation: self.allocation,
        })
    }
}
