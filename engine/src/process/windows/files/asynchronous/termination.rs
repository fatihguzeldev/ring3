use super::{completion::status, context::AsyncStatus32};
use crate::{
    files::{FileError32, OpenFileId},
    memory::{Access, AddressSpace, GuestAddress, PAGE_SIZE, PageRange, WordWrite32},
    process::windows::files::FileRuntime32,
    windows::synchronization::EventBinding32,
};
pub(in crate::process) struct PreparedFileThreadExit32 {
    operations: Vec<u64>,
    revision: u64,
    words: Vec<WordWrite32>,
    bindings: Vec<EventBinding32>,
    copy: super::super::ordinary::PreparedCopyThreadExit32,
}
impl PreparedFileThreadExit32 {
    pub(in crate::process) fn words(&self) -> &[WordWrite32] {
        &self.words
    }
    pub(in crate::process) fn event_handles(&self) -> &[u32] {
        self.copy.event_handles()
    }
    pub(in crate::process) fn bindings(&self) -> &[EventBinding32] {
        &self.bindings
    }
}
impl FileRuntime32 {
    pub(in crate::process) fn prepare_thread_exit(
        &self,
        memory: &AddressSpace,
        thread: u32,
        unmaps: &[PageRange],
        saved: &[&super::FileThreadReply32],
        active: bool,
    ) -> Result<PreparedFileThreadExit32, FileError32> {
        let mut ordinary = Vec::new();
        ordinary
            .try_reserve_exact(saved.len())
            .map_err(|_| FileError32::Allocation)?;
        ordinary.extend(saved.iter().map(|reply| reply.ordinary_reply()));
        let copy = self.prepare_copy_thread_exit(&ordinary, active)?;
        let count = self
            .asynchronous
            .operations
            .iter()
            .filter(|operation| {
                operation.thread == thread && operation.status == AsyncStatus32::Pending
            })
            .count();
        let mut operations = Vec::new();
        operations
            .try_reserve_exact(count)
            .map_err(|_| FileError32::Allocation)?;
        let mut closes: Vec<(OpenFileId, u32)> = Vec::new();
        closes
            .try_reserve_exact(count)
            .map_err(|_| FileError32::Allocation)?;
        let mut words = Vec::new();
        words
            .try_reserve_exact(count * 2)
            .map_err(|_| FileError32::Allocation)?;
        let binding_count = self
            .asynchronous
            .operations
            .iter()
            .filter(|operation| {
                operation.thread == thread && operation.status == AsyncStatus32::Pending
            })
            .map(|operation| operation.events.len())
            .sum();
        let mut bindings = Vec::new();
        bindings
            .try_reserve_exact(binding_count)
            .map_err(|_| FileError32::Allocation)?;
        for operation in &self.asynchronous.operations {
            if operation.thread != thread || operation.status != AsyncStatus32::Pending {
                continue;
            }
            operations.push(operation.id);
            bindings.extend_from_slice(&operation.events);
            if let Some((_, count)) = closes.iter_mut().find(|(open, _)| *open == operation.open) {
                *count = count.checked_add(1).ok_or(FileError32::Exhausted)?;
            } else {
                closes.try_reserve(1).map_err(|_| FileError32::Allocation)?;
                closes.push((operation.open, 1));
            }
            for (offset, value) in [(0, status(995)), (4, 0)] {
                let address = GuestAddress(
                    operation
                        .overlapped
                        .0
                        .checked_add(offset)
                        .ok_or(FileError32::NoAccess)?,
                );
                if unmaps.iter().any(|range| {
                    (range.first as u64) * u64::from(PAGE_SIZE) < u64::from(address.0) + 4
                        && u64::from(address.0)
                            < ((range.first + range.count) as u64) * u64::from(PAGE_SIZE)
                }) {
                    continue;
                }
                memory
                    .check_span(address, 4, Access::Write)
                    .map_err(|_| FileError32::NoAccess)?;
                let first = address.0 / PAGE_SIZE * PAGE_SIZE;
                let last = (address.0 + 3) / PAGE_SIZE * PAGE_SIZE;
                for page in [first, last] {
                    let expected = operation
                        .mappings
                        .iter()
                        .find(|(base, _)| base.0 == page)
                        .map(|(_, version)| *version)
                        .ok_or(FileError32::NoAccess)?;
                    if memory.mapping_version_at(GuestAddress(page)) != Ok(expected) {
                        return Err(FileError32::NoAccess);
                    }
                }
                words.push(WordWrite32 { address, value });
            }
        }
        words.sort_unstable_by_key(|word| word.address.0);
        if words
            .windows(2)
            .any(|pair| u64::from(pair[0].address.0) + 4 > u64::from(pair[1].address.0))
        {
            return Err(FileError32::InvalidParameter);
        }
        for &(open, count) in copy.closes() {
            if let Some((_, total)) = closes.iter_mut().find(|(candidate, _)| *candidate == open) {
                *total = total.checked_add(count).ok_or(FileError32::Exhausted)?;
            } else {
                closes.try_reserve(1).map_err(|_| FileError32::Allocation)?;
                closes.push((open, count));
            }
        }
        self.vfs.check_close_open_group(&closes, 0, &[])?;
        Ok(PreparedFileThreadExit32 {
            operations,
            revision: self.vfs.revision(),
            words,
            bindings,
            copy,
        })
    }
    pub(in crate::process) fn commit_thread_exit(
        &mut self,
        thread: u32,
        prepared: PreparedFileThreadExit32,
    ) {
        assert_eq!(self.vfs.revision(), prepared.revision);
        for id in prepared.operations {
            let index = self
                .asynchronous
                .operations
                .iter()
                .position(|operation| operation.id == id)
                .unwrap();
            let operation = &self.asynchronous.operations[index];
            assert_eq!(operation.status, AsyncStatus32::Pending);
            let open = operation.open;
            self.vfs.cancel_operation_io(id);
            self.vfs
                .close_open(open)
                .expect("all thread-operation source references were preflighted together");
            if self.vfs.open_info(open).is_err() {
                self.release_file_locks(open);
            }
            let operation = &mut self.asynchronous.operations[index];
            operation.events.clear();
            if let Some(bytes) = operation.write.take() {
                self.asynchronous.retained_bytes -= bytes.len();
            }
            operation.cancel = true;
            operation.status = AsyncStatus32::Complete {
                error: 995,
                bytes: 0,
            };
        }
        self.commit_copy_thread_exit(prepared.copy);
        self.vfs.cancel_guest_thread_io(thread);
    }
}
