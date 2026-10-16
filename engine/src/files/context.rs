use super::{
    content::ContentCache32,
    path,
    tree::{Content32, NamespaceEntry32, Node32},
    types::*,
};

pub(crate) struct Vfs32 {
    pub(super) epoch: u64,
    pub(super) revision: u64,
    pub(super) now: u64,
    pub(super) limits: FileLimits32,
    pub(super) nodes: Vec<Option<Node32>>,
    pub(super) node_index: Vec<(FileId, usize)>,
    pub(super) entries: Vec<NamespaceEntry32>,
    pub(super) opens: Vec<Option<super::handles::Open32>>,
    pub(super) cache: ContentCache32,
    pub(super) cwd: Vec<u16>,
    pub(super) drive_dirs: [Option<Vec<u16>>; 26],
    pub(super) next_file: u64,
    pub(super) next_open: u64,
    pub(super) overlay_pages: usize,
    pub(super) deleted: Vec<NamespaceEntry32>,
    pub(super) flushed_revision: u64,
    pub(super) io_thread: u32,
    pub(super) pending_writes: Vec<super::io::WriteStage32>,
}
impl Vfs32 {
    pub(crate) fn new(epoch: u64, limits: FileLimits32) -> Result<Self, FileError32> {
        if epoch == 0
            || limits.nodes == 0
            || limits.nodes > 1_048_576
            || limits.opens == 0
            || limits.opens > 1_048_576
            || limits.cache_pages == 0
            || limits.cache_pages > 65_536
            || limits.overlay_pages.checked_mul(FILE_PAGE_BYTES).is_none()
            || limits.requests == 0
            || limits.requests > 4096
            || limits.max_io_bytes == 0
            || limits.max_io_bytes > 64 * 1024 * 1024
        {
            return Err(FileError32::InvalidLimits);
        }
        let mut nodes = Vec::new();
        nodes
            .try_reserve_exact(limits.nodes)
            .map_err(|_| FileError32::Allocation)?;
        let mut node_index = Vec::new();
        node_index
            .try_reserve_exact(limits.nodes)
            .map_err(|_| FileError32::Allocation)?;
        node_index.push((FileId(1), 0));
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(limits.nodes)
            .map_err(|_| FileError32::Allocation)?;
        let mut opens = Vec::new();
        opens
            .try_reserve_exact(limits.opens)
            .map_err(|_| FileError32::Allocation)?;
        let mut deleted = Vec::new();
        deleted
            .try_reserve_exact(limits.nodes)
            .map_err(|_| FileError32::Allocation)?;
        let mut pending_writes = Vec::new();
        pending_writes
            .try_reserve_exact(limits.requests)
            .map_err(|_| FileError32::Allocation)?;
        let root = vec![67, 58, 92];
        nodes.push(Some(Node32 {
            id: FileId(1),
            attributes: ATTR_DIRECTORY,
            times: FileTimes32::default(),
            changed: 0,
            size: 0,
            linked: true,
            delete_pending: false,
            descriptions: 0,
            content_revision: 1,
            dirty: false,
            content: Content32::default(),
            security: crate::windows::security::SecurityDescriptor32::for_token(
                &crate::windows::security::Token32::virtual_process(),
            )?,
        }));
        entries.push(NamespaceEntry32 {
            key: root.clone(),
            path: root.clone(),
            file: FileId(1),
        });
        let mut drive_dirs = std::array::from_fn(|_| None);
        drive_dirs[2] = Some(root.clone());
        Ok(Self {
            epoch,
            revision: 1,
            now: 0,
            limits,
            nodes,
            node_index,
            entries,
            opens,
            cache: ContentCache32::new(limits.cache_pages, limits.requests)?,
            cwd: root,
            drive_dirs,
            next_file: 2,
            next_open: 1,
            overlay_pages: 0,
            deleted,
            flushed_revision: 0,
            pending_writes,
            io_thread: 0,
        })
    }
    pub(crate) fn set_io_thread(&mut self, thread: u32) {
        self.io_thread = thread;
    }
    pub(crate) fn epoch(&self) -> u64 {
        self.epoch
    }
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }
    pub(crate) fn max_io_bytes(&self) -> usize {
        self.limits.max_io_bytes
    }
    pub(crate) fn set_time(&mut self, filetime: u64) -> Result<(), FileError32> {
        self.now = filetime;
        Ok(())
    }
    pub(crate) fn current_directory(&self) -> &[u16] {
        &self.cwd
    }
    pub(crate) fn full_path(&self, input: &[u16]) -> Result<Vec<u16>, FileError32> {
        self.normalize(input, false)
    }
    pub(super) fn normalize(&self, input: &[u16], wildcard: bool) -> Result<Vec<u16>, FileError32> {
        let cwd = if input.get(1) == Some(&58) {
            let drive = (input[0] as u8).to_ascii_uppercase();
            if drive.is_ascii_uppercase() {
                self.drive_dirs[(drive - b'A') as usize]
                    .as_deref()
                    .unwrap_or(&self.cwd)
            } else {
                &self.cwd
            }
        } else {
            &self.cwd
        };
        path::normalize(input, cwd, wildcard)
    }
    pub(crate) fn volume(&self) -> FileVolume32 {
        FileVolume32 {
            serial: 0x5233_0001,
            readonly: false,
            total_bytes: (self.limits.overlay_pages as u64) * FILE_PAGE_BYTES as u64,
            free_bytes: ((self.limits.overlay_pages - self.overlay_pages) as u64)
                * FILE_PAGE_BYTES as u64,
            max_component: 255,
        }
    }
    pub(super) fn stamp(&self) -> Result<(u64, u64), FileError32> {
        Ok((
            self.revision,
            self.revision.checked_add(1).ok_or(FileError32::Exhausted)?,
        ))
    }
    pub(super) fn validate(&self, epoch: u64, revision: u64) -> Result<(), FileError32> {
        if self.epoch != epoch || self.revision != revision {
            Err(FileError32::Stale)
        } else {
            Ok(())
        }
    }
}
