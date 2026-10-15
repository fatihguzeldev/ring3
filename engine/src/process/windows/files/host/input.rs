use super::*;

pub(super) enum FileInputKind32 {
    Mount(FileMountStart32),
    Environment(Vec<u32>),
}
pub(super) struct PendingFileInput32 {
    pub epoch: u64,
    pub token: u64,
    pub total: u32,
    pub units: Vec<u16>,
    pub kind: FileInputKind32,
}
impl PendingFileInput32 {
    pub fn status(&self) -> FileInput32 {
        FileInput32 {
            epoch: self.epoch,
            token: self.token,
            kind: match self.kind {
                FileInputKind32::Mount(_) => 1,
                FileInputKind32::Environment(_) => 2,
            },
            received: self.units.len() as u32,
            total: self.total,
        }
    }
}
pub(in crate::process::windows::files) struct HostFileInput32 {
    next: u64,
    pub(super) pending: Option<PendingFileInput32>,
}
impl Default for HostFileInput32 {
    fn default() -> Self {
        Self {
            next: 1,
            pending: None,
        }
    }
}
impl HostFileInput32 {
    pub(super) fn begin(
        &mut self,
        epoch: u64,
        total: u32,
        kind: FileInputKind32,
    ) -> Result<FileInput32, FileError32> {
        if self.pending.is_some() {
            return Err(FileError32::Busy);
        }
        let next = self.next.checked_add(1).ok_or(FileError32::Exhausted)?;
        let mut units = Vec::new();
        units
            .try_reserve_exact(total as usize)
            .map_err(|_| FileError32::Allocation)?;
        let pending = PendingFileInput32 {
            epoch,
            token: self.next,
            total,
            units,
            kind,
        };
        let status = pending.status();
        self.pending = Some(pending);
        self.next = next;
        Ok(status)
    }
    pub(super) fn checked(
        &self,
        epoch: u64,
        token: u64,
    ) -> Result<&PendingFileInput32, FileError32> {
        self.pending
            .as_ref()
            .filter(|p| p.epoch == epoch && p.token == token)
            .ok_or(FileError32::Stale)
    }
    fn append(
        &mut self,
        epoch: u64,
        token: u64,
        offset: u32,
        units: &[u16],
    ) -> Result<FileInput32, FileError32> {
        let pending = self.checked(epoch, token)?;
        if units.is_empty()
            || units.len() > 2048
            || offset as usize != pending.units.len()
            || pending
                .units
                .len()
                .checked_add(units.len())
                .ok_or(FileError32::InvalidParameter)?
                > pending.total as usize
        {
            return Err(FileError32::InvalidParameter);
        }
        let pending = self.pending.as_mut().unwrap();
        pending.units.extend_from_slice(units);
        Ok(pending.status())
    }
}
impl EngineInstance {
    pub fn pending_file_input(&self) -> Result<Option<FileInput32>, HostError> {
        self.memory()?;
        let files = self
            .files
            .as_ref()
            .ok_or(HostError::File(FileError32::Unsupported))?;
        Ok(files
            .host_input
            .pending
            .as_ref()
            .map(PendingFileInput32::status))
    }
    pub fn append_file_input(
        &mut self,
        epoch: u64,
        token: u64,
        offset: u32,
        units: &[u16],
    ) -> Result<FileInput32, HostError> {
        let files = self.files_mut()?;
        if files.vfs.epoch() != epoch {
            return Err(HostError::File(FileError32::Stale));
        }
        files
            .host_input
            .append(epoch, token, offset, units)
            .map_err(HostError::File)
    }
    pub fn abort_file_input(&mut self, epoch: u64, token: u64) -> Result<(), HostError> {
        let files = self.files_mut()?;
        files
            .host_input
            .checked(epoch, token)
            .map_err(HostError::File)?;
        files.host_input.pending = None;
        Ok(())
    }
}
