use super::context::*;
use crate::{
    files::{FileAccess32, FileError32},
    memory::{Access, AddressSpace, GuestAddress, PAGE_SIZE},
    process::windows::files::{
        FileRuntime32,
        ordinary::{FileCallOutcome32, memory as guest},
    },
    windows::{files::FileApi32, synchronization::SynchronizationContext32},
};

impl FileRuntime32 {
    pub(in crate::process::windows::files) fn execute_overlapped(
        &mut self,
        memory: &mut AddressSpace,
        sync: &mut SynchronizationContext32,
        thread: u32,
        file_event: u32,
        api: FileApi32,
        args: &[u32],
    ) -> Result<FileCallOutcome32, FileError32> {
        let open = self.handles.file(args[0])?;
        let file = self.vfs.file_for_open(open)?;
        let write = api == FileApi32::WriteFile;
        if !matches!(api, FileApi32::ReadFile | FileApi32::WriteFile) || args[4] == 0 {
            return Err(FileError32::InvalidParameter);
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
        let overlapped = GuestAddress(args[4]);
        let bytes = guest::read::<20>(memory, args[4])?;
        let mut offset = u64::from(u32::from_le_bytes(bytes[8..12].try_into().unwrap()))
            | u64::from(u32::from_le_bytes(bytes[12..16].try_into().unwrap())) << 32;
        if write && rights & FileAccess32::WRITE.0 == 0 {
            offset = u64::MAX;
        }
        if offset != u64::MAX && offset.checked_add(u64::from(args[2])).is_none()
            || (!write && offset == u64::MAX)
        {
            return Err(FileError32::InvalidParameter);
        }
        let aligned_offset = if offset == u64::MAX {
            self.vfs.info(file)?.size
        } else {
            offset
        };
        super::positioned::check_alignment(
            self.vfs.open_options(open)?.no_buffering,
            args[1],
            args[2],
            aligned_offset,
        )?;
        guest::output(memory, args[4], 8)?;
        if args[3] != 0 {
            guest::output(memory, args[3], 4)?;
        }
        memory
            .check_span(
                GuestAddress(args[1]),
                args[2] as usize,
                if write { Access::Read } else { Access::Write },
            )
            .map_err(|_| FileError32::NoAccess)?;
        let old = self.asynchronous.find(open, overlapped).or_else(|| {
            (self.asynchronous.operations.len() >= self.asynchronous.max_operations)
                .then(|| {
                    self.asynchronous.operations.iter().position(|operation| {
                        matches!(operation.status, AsyncStatus32::Complete { .. })
                    })
                })
                .flatten()
        });
        if old.is_some_and(|index| {
            self.asynchronous.operations[index].status == AsyncStatus32::Pending
        }) {
            return Err(FileError32::Busy);
        }
        if self.asynchronous.operations.len() - usize::from(old.is_some())
            >= self.asynchronous.max_operations
        {
            return Err(FileError32::Capacity);
        }
        let retained = self
            .asynchronous
            .retained_bytes
            .checked_add(if write { args[2] as usize } else { 0 })
            .ok_or(FileError32::Capacity)?;
        if retained > self.asynchronous.max_bytes {
            return Err(FileError32::Capacity);
        }
        let next = self
            .asynchronous
            .next
            .checked_add(1)
            .ok_or(FileError32::Exhausted)?;
        self.vfs.check_retain_open(open)?;
        let data = if write {
            let mut data = Vec::new();
            data.try_reserve_exact(args[2] as usize)
                .map_err(|_| FileError32::Allocation)?;
            data.resize(args[2] as usize, 0);
            memory
                .read(GuestAddress(args[1]), &mut data)
                .map_err(|_| FileError32::NoAccess)?;
            Some(data)
        } else {
            None
        };
        let mut mappings = Vec::new();
        capture_mappings(memory, overlapped, 8, &mut mappings)?;
        if !write {
            capture_mappings(
                memory,
                GuestAddress(args[1]),
                args[2] as usize,
                &mut mappings,
            )?;
        }
        let event = u32::from_le_bytes(bytes[16..20].try_into().unwrap()) & !1;
        let handles = [file_event, event];
        let handles = if event == 0 {
            &handles[..1]
        } else {
            &handles[..]
        };
        let release = old.map_or(&[][..], |index| {
            self.asynchronous.operations[index].events.as_slice()
        });
        let events = sync
            .prepare_io_events(handles, release, handles)
            .map_err(super::synchronization_error)?;
        memory
            .ensure_transaction_versions(1)
            .map_err(|_| FileError32::Exhausted)?;
        let mut pending = [0; 8];
        pending[..4].copy_from_slice(&0x103u32.to_le_bytes());
        let zero = 0u32.to_le_bytes();
        let spans = [
            (overlapped, pending.as_slice()),
            (
                GuestAddress(args[3]),
                if args[3] == 0 {
                    &[][..]
                } else {
                    zero.as_slice()
                },
            ),
        ];
        memory
            .write_spans(&spans)
            .map_err(|_| FileError32::NoAccess)?;
        let bindings = sync.commit_event_bindings(events);
        self.vfs
            .retain_open(open)
            .expect("open retention checked before overlapped acceptance");
        if let Some(index) = old {
            let old = self.asynchronous.operations.remove(index);
            let _ = old;
        }
        let id = self.asynchronous.next;
        self.asynchronous.next = next;
        self.asynchronous.retained_bytes = retained;
        self.asynchronous.operations.push(AsyncOperation32 {
            id,
            open,
            file,
            thread,
            buffer: GuestAddress(args[1]),
            length: args[2],
            overlapped,
            offset,
            write: data,
            events: bindings,
            status: AsyncStatus32::Pending,
            cancel: false,
            mappings,
            lock: None,
        });
        Ok(FileCallOutcome32::error(0, 997))
    }
}
pub(super) fn capture_mappings(
    memory: &AddressSpace,
    address: GuestAddress,
    length: usize,
    output: &mut Vec<(GuestAddress, u64)>,
) -> Result<(), FileError32> {
    if length == 0 {
        return Ok(());
    }
    let first = address.0 / PAGE_SIZE;
    let end = (u64::from(address.0) + length as u64).div_ceil(u64::from(PAGE_SIZE));
    output
        .try_reserve_exact(end as usize - first as usize)
        .map_err(|_| FileError32::Allocation)?;
    for page in u64::from(first)..end {
        let at = GuestAddress(page as u32 * PAGE_SIZE);
        output.push((
            at,
            memory
                .mapping_version_at(at)
                .map_err(|_| FileError32::NoAccess)?,
        ));
    }
    Ok(())
}
