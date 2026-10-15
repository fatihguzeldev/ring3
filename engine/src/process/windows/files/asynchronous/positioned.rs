use crate::{
    files::{FileAccess32, FileError32, FileIo32},
    memory::{Access, AddressSpace, GuestAddress},
    process::windows::files::{
        FileRuntime32,
        ordinary::{FileCallOutcome32, memory as guest},
    },
    windows::files::FileApi32,
};

impl FileRuntime32 {
    pub(in crate::process::windows::files) fn execute_positioned(
        &mut self,
        memory: &mut AddressSpace,
        api: FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        let open = self.handles.file(args[0])?;
        let write = api == FileApi32::WriteFile;
        if !matches!(api, FileApi32::ReadFile | FileApi32::WriteFile) || args[4] == 0 {
            return Err(FileError32::InvalidParameter);
        }
        let raw = guest::read::<20>(memory, args[4])?;
        guest::output(memory, args[4], 8)?;
        if args[3] != 0 {
            guest::output(memory, args[3], 4)?;
        }
        let rights = self.handles.access(args[0])?;
        if if write {
            rights & (FileAccess32::WRITE.0 | FileAccess32::APPEND.0) == 0
        } else {
            rights & FileAccess32::READ.0 == 0
        } {
            return Err(FileError32::AccessDenied);
        }
        if args[2] as usize > self.asynchronous.max_bytes {
            return Err(FileError32::Capacity);
        }
        let offset = u64::from(u32::from_le_bytes(raw[8..12].try_into().unwrap()))
            | u64::from(u32::from_le_bytes(raw[12..16].try_into().unwrap())) << 32;
        let offset = if write && (offset == u64::MAX || rights & FileAccess32::WRITE.0 == 0) {
            self.vfs.info(self.vfs.file_for_open(open)?)?.size
        } else {
            offset
        };
        if offset.checked_add(u64::from(args[2])).is_none() {
            return Err(FileError32::InvalidParameter);
        }
        check_alignment(
            self.vfs.open_options(open)?.no_buffering,
            args[1],
            args[2],
            offset,
        )?;
        self.check_file_lock(open, offset, u64::from(args[2]), write)?;
        memory
            .check_span(
                GuestAddress(args[1]),
                args[2] as usize,
                if write { Access::Read } else { Access::Write },
            )
            .map_err(|_| FileError32::NoAccess)?;
        if write {
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(args[2] as usize)
                .map_err(|_| FileError32::Allocation)?;
            bytes.resize(args[2] as usize, 0);
            memory
                .read(GuestAddress(args[1]), &mut bytes)
                .map_err(|_| FileError32::NoAccess)?;
            let prepared = match self.vfs.prepare_write_positioned(open, offset, &bytes)? {
                FileIo32::NeedBytes(request) => {
                    return Ok(FileCallOutcome32::PendingBytes(request));
                }
                FileIo32::Ready(prepared) => prepared,
            };
            prepared.validate(&self.vfs)?;
            let count = prepared.length() as u32;
            let mut internal = [0; 8];
            internal[4..].copy_from_slice(&count.to_le_bytes());
            let count = count.to_le_bytes();
            memory
                .write_spans(&[
                    (GuestAddress(args[4]), &internal),
                    (
                        GuestAddress(args[3]),
                        if args[3] == 0 { &[][..] } else { &count },
                    ),
                ])
                .map_err(|_| FileError32::NoAccess)?;
            self.vfs.commit_write(prepared);
        } else {
            let prepared = match self
                .vfs
                .prepare_read_positioned(open, offset, args[2] as usize)?
            {
                FileIo32::NeedBytes(request) => {
                    return Ok(FileCallOutcome32::PendingBytes(request));
                }
                FileIo32::Ready(prepared) => prepared,
            };
            prepared.validate(&self.vfs)?;
            let count = prepared.bytes().len() as u32;
            let mut internal = [0; 8];
            internal[4..].copy_from_slice(&count.to_le_bytes());
            let count = count.to_le_bytes();
            memory
                .write_spans(&[
                    (GuestAddress(args[4]), &internal),
                    (
                        GuestAddress(args[3]),
                        if args[3] == 0 { &[][..] } else { &count },
                    ),
                    (GuestAddress(args[1]), prepared.bytes()),
                ])
                .map_err(|_| FileError32::NoAccess)?;
            self.vfs.commit_read(prepared);
        }
        Ok(FileCallOutcome32::success(1))
    }
}

pub(super) fn check_alignment(
    no_buffering: bool,
    buffer: u32,
    length: u32,
    offset: u64,
) -> Result<(), FileError32> {
    if no_buffering
        && (!buffer.is_multiple_of(512)
            || !length.is_multiple_of(512)
            || !offset.is_multiple_of(512))
    {
        return Err(FileError32::InvalidParameter);
    }
    Ok(())
}
