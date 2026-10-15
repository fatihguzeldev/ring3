use super::protection::MappingProtection32;
use crate::{
    files::{FileError32, FileId, OpenFileId},
    memory::{PageRange, SharedPages},
};

#[derive(Clone, Copy, Debug)]
pub(in crate::process::windows::files) struct MappingLimits32 {
    pub objects: usize,
    pub views: usize,
}
pub(in crate::process::windows::files) struct MappingRuntime32 {
    pub(super) objects: Vec<MappingObject32>,
    pub(super) views: Vec<MappingView32>,
    pub(super) pages: Vec<MappingPage32>,
    pub(super) limits: MappingLimits32,
    pub(super) bounds: PageRange,
    pub(super) next: u64,
    pub(super) pending: Option<PendingView32>,
}
pub(super) struct MappingObject32 {
    pub id: u64,
    pub source: Option<OpenFileId>,
    pub key: MappingBacking32,
    pub size: u64,
    pub protection: MappingProtection32,
    pub name: Option<Vec<u16>>,
    pub references: u32,
    pub handles: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MappingBacking32 {
    File(FileId),
    PageFile(u64),
}
pub(super) struct MappingPage32 {
    pub key: MappingBacking32,
    pub offsets: Vec<u64>,
    pub backing: SharedPages,
    pub flushed: Vec<u64>,
    pub committed: Vec<bool>,
}
pub(super) struct MappingView32 {
    pub object: u64,
    pub address: u32,
    pub length: usize,
    pub offset: u64,
    pub copy: bool,
    pub range: PageRange,
    pub permissions: crate::memory::Permissions,
}
impl MappingRuntime32 {
    pub(in crate::process::windows::files) fn new(
        limits: MappingLimits32,
        bounds: PageRange,
    ) -> Result<Self, FileError32> {
        if limits.objects == 0
            || limits.views == 0
            || limits.objects > 65_536
            || limits.views > 65_536
        {
            return Err(FileError32::InvalidLimits);
        }
        let mut objects = Vec::new();
        objects
            .try_reserve_exact(limits.objects)
            .map_err(|_| FileError32::Allocation)?;
        let mut views = Vec::new();
        views
            .try_reserve_exact(limits.views)
            .map_err(|_| FileError32::Allocation)?;
        Ok(Self {
            objects,
            views,
            pages: Vec::new(),
            limits,
            bounds,
            next: 1,
            pending: None,
        })
    }
    pub(super) fn index(&self, id: u64) -> Result<usize, FileError32> {
        self.objects
            .iter()
            .position(|object| object.id == id)
            .ok_or(FileError32::InvalidHandle)
    }
    pub(in crate::process::windows::files) fn retain(
        &mut self,
        id: u64,
    ) -> Result<(), FileError32> {
        let index = self.index(id)?;
        let object = &mut self.objects[index];
        let handles = object
            .handles
            .checked_add(1)
            .ok_or(FileError32::Exhausted)?;
        let references = object
            .references
            .checked_add(1)
            .ok_or(FileError32::Exhausted)?;
        object.handles = handles;
        object.references = references;
        Ok(())
    }
}

pub(super) struct PendingView32 {
    pub object: u64,
    pub access: u32,
    pub offset: u64,
    pub length: usize,
    pub base: u32,
    pub missing: Vec<u64>,
    pub bytes: Vec<u8>,
    pub filled: usize,
}

impl MappingRuntime32 {
    pub(in crate::process::windows::files) fn cancel_pending(&mut self) {
        self.pending = None;
    }
}
