use super::super::{
    FileCallOutcome32, FileRuntime32,
    asynchronous::{AsyncKind32, AsyncOperation32, AsyncStatus32, AsyncTarget32, capture_mappings},
    ordinary::memory as guest,
};
use crate::{
    files::FileError32,
    memory::{Access, AddressSpace, GuestAddress},
    windows::{
        files::{completion::CompletionTarget32, pipes::PipeEndpointId32},
        synchronization::{PreparedEventBindings32, SynchronizationContext32},
    },
};

pub(super) struct PipeOperation32 {
    pub pipe: PipeEndpointId32,
    pub thread: u32,
    pub buffer: u32,
    pub length: u32,
    pub count: u32,
    pub overlapped: u32,
    pub kind: AsyncKind32,
}
struct PreparedPipeOperation32 {
    operation: AsyncOperation32,
    old: Option<usize>,
    next: u64,
    retained: usize,
    events: Option<PreparedEventBindings32>,
}
impl FileRuntime32 {
    pub(super) fn accept_pipe_operation(
        &mut self,
        memory: &mut AddressSpace,
        sync: Option<&mut SynchronizationContext32>,
        request: PipeOperation32,
    ) -> Result<u64, FileError32> {
        let prepared = self.prepare_pipe_operation(memory, sync.as_deref(), request, None)?;
        Self::publish_pipe_operation(memory, &prepared)?;
        Ok(self.commit_pipe_operation(sync, prepared))
    }
    fn prepare_pipe_operation(
        &self,
        memory: &AddressSpace,
        sync: Option<&SynchronizationContext32>,
        request: PipeOperation32,
        client: Option<&crate::windows::files::pipes::PreparedPipeClient32>,
    ) -> Result<PreparedPipeOperation32, FileError32> {
        let PipeOperation32 {
            pipe,
            thread,
            buffer,
            length,
            count,
            overlapped,
            kind,
        } = request;
        let asynchronous = match client {
            Some(client) if client.endpoint() == pipe => client.overlapped(),
            Some(_) => return Err(FileError32::InvalidParameter),
            None => self.pipes.options(pipe)?.overlapped,
        };
        if asynchronous && overlapped == 0 {
            return Err(FileError32::InvalidParameter);
        }
        if !asynchronous
            && matches!(
                kind,
                AsyncKind32::Read | AsyncKind32::Write(_) | AsyncKind32::Transact { .. }
            )
            && count == 0
        {
            return Err(FileError32::InvalidParameter);
        }
        let target = CompletionTarget32::Pipe(pipe);
        let pointer = (overlapped != 0).then_some(GuestAddress(overlapped));
        let old = pointer
            .and_then(|pointer| self.asynchronous.find_description(target, pointer))
            .or_else(|| {
                (self.asynchronous.operations.len() >= self.asynchronous.max_operations)
                    .then(|| {
                        self.asynchronous
                            .operations
                            .iter()
                            .position(|op| op.status != AsyncStatus32::Pending)
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
        let retained = match &kind {
            AsyncKind32::Write(bytes) | AsyncKind32::Transact { input: bytes, .. } => bytes.len(),
            _ => 0,
        };
        let total = self
            .asynchronous
            .retained_bytes
            .checked_add(retained)
            .ok_or(FileError32::Capacity)?;
        if total > self.asynchronous.max_bytes || length as usize > self.asynchronous.max_bytes {
            return Err(FileError32::Capacity);
        }
        let next = self
            .asynchronous
            .next
            .checked_add(1)
            .ok_or(FileError32::Exhausted)?;
        let read = matches!(kind, AsyncKind32::Read | AsyncKind32::Transact { .. });
        if read {
            guest::output(memory, buffer, length as usize)?;
        }
        if count != 0 {
            guest::output(memory, count, 4)?;
        }
        let mut mappings = Vec::new();
        if read {
            capture_mappings(memory, GuestAddress(buffer), length as usize, &mut mappings)?;
        }
        if count != 0 {
            capture_mappings(memory, GuestAddress(count), 4, &mut mappings)?;
        }
        let mut handles = [0; 2];
        let mut handle_count = 0;
        let mut skip_port = false;
        if let Some(pointer) = pointer {
            guest::output(memory, pointer.0, 8)?;
            let raw = guest::read::<20>(memory, pointer.0)?;
            capture_mappings(memory, pointer, 8, &mut mappings)?;
            let event = u32::from_le_bytes(raw[16..].try_into().unwrap());
            skip_port = event & 1 != 0;
            if event & !1 != 0 {
                handles[handle_count] = event & !1;
                handle_count += 1;
            }
        }
        if let Some((_, event)) = self.events.iter().find(|(id, _)| *id == target)
            && self.completion.modes(target) & 2 == 0
        {
            handles[handle_count] = *event;
            handle_count += 1;
        }
        if client.is_none() {
            self.pipes.check_retain(pipe)?;
        }
        let release = old.map_or(&[][..], |index| {
            self.asynchronous.operations[index].events.as_slice()
        });
        let events = match sync {
            Some(sync) => Some(
                sync.prepare_io_events(&handles[..handle_count], release, &handles[..handle_count])
                    .map_err(super::super::asynchronous::synchronization_error)?,
            ),
            None if handle_count == 0 && release.is_empty() => None,
            None => return Err(FileError32::Unsupported),
        };
        Ok(PreparedPipeOperation32 {
            old,
            next,
            retained: total,
            events,
            operation: AsyncOperation32 {
                id: self.asynchronous.next,
                target: AsyncTarget32::Pipe(pipe),
                thread,
                buffer: GuestAddress(buffer),
                length,
                overlapped: pointer,
                count_pointer: (count != 0).then_some(GuestAddress(count)),
                offset: 0,
                kind,
                events: Vec::new(),
                status: AsyncStatus32::Pending,
                cancel: false,
                mappings,
                skip_port,
            },
        })
    }
    fn publish_pipe_operation(
        memory: &mut AddressSpace,
        prepared: &PreparedPipeOperation32,
    ) -> Result<(), FileError32> {
        let operation = &prepared.operation;
        let mut pending = [0; 8];
        pending[..4].copy_from_slice(&0x103u32.to_le_bytes());
        let zero = [0; 4];
        if operation.overlapped.is_some() || operation.count_pointer.is_some() {
            memory
                .ensure_transaction_versions(1)
                .map_err(|_| FileError32::Exhausted)?;
            memory
                .write_spans(&[
                    (
                        operation.overlapped.unwrap_or(GuestAddress(0)),
                        if operation.overlapped.is_some() {
                            pending.as_slice()
                        } else {
                            &[]
                        },
                    ),
                    (
                        operation.count_pointer.unwrap_or(GuestAddress(0)),
                        if operation.count_pointer.is_some() {
                            zero.as_slice()
                        } else {
                            &[]
                        },
                    ),
                ])
                .map_err(|_| FileError32::NoAccess)?;
        }
        Ok(())
    }
    fn commit_pipe_operation(
        &mut self,
        sync: Option<&mut SynchronizationContext32>,
        mut prepared: PreparedPipeOperation32,
    ) -> u64 {
        prepared.operation.events = match prepared.events {
            Some(events) => sync.unwrap().commit_event_bindings(events),
            None => Vec::new(),
        };
        let AsyncTarget32::Pipe(pipe) = prepared.operation.target else {
            unreachable!()
        };
        self.pipes
            .retain(pipe)
            .expect("pipe retention preflighted before guest publication");
        if let Some(index) = prepared.old {
            self.asynchronous.operations.remove(index);
        }
        let id = prepared.operation.id;
        self.asynchronous.next = prepared.next;
        self.asynchronous.retained_bytes = prepared.retained;
        self.asynchronous.operations.push(prepared.operation);
        id
    }
    pub(super) fn accept_pipe_call(
        &mut self,
        memory: &mut AddressSpace,
        sync: Option<&mut SynchronizationContext32>,
        client: crate::windows::files::pipes::PreparedPipeClient32,
        request: PipeOperation32,
    ) -> Result<u64, FileError32> {
        let prepared =
            self.prepare_pipe_operation(memory, sync.as_deref(), request, Some(&client))?;
        Self::publish_pipe_operation(memory, &prepared)?;
        self.pipes.commit_client(client);
        Ok(self.commit_pipe_operation(sync, prepared))
    }
    pub(super) fn progress_pipe_id(
        &mut self,
        memory: &mut AddressSpace,
        sync: Option<&mut SynchronizationContext32>,
        id: u64,
    ) -> Result<FileCallOutcome32, FileError32> {
        let index = self
            .asynchronous
            .operations
            .iter()
            .position(|op| op.id == id)
            .ok_or(FileError32::InvalidHandle)?;
        if self.asynchronous.operations[index].status == AsyncStatus32::Pending {
            self.progress_pipe_operation(memory, sync, index)?;
        }
        self.pipe_operation_result(id)
    }
    pub(super) fn pipe_operation_result(
        &mut self,
        id: u64,
    ) -> Result<FileCallOutcome32, FileError32> {
        let op = self
            .asynchronous
            .operations
            .iter()
            .find(|op| op.id == id)
            .ok_or(FileError32::InvalidHandle)?;
        Ok(match op.status {
            AsyncStatus32::Pending => FileCallOutcome32::Pending,
            AsyncStatus32::Complete { error: 0, .. } => {
                self.pipe_wait = None;
                FileCallOutcome32::success(1)
            }
            AsyncStatus32::Complete { error, .. } => {
                self.pipe_wait = None;
                FileCallOutcome32::error(0, error)
            }
        })
    }
    pub(super) fn pipe_input_bytes(
        &self,
        memory: &AddressSpace,
        address: u32,
        length: u32,
    ) -> Result<Vec<u8>, FileError32> {
        if length as usize > self.asynchronous.max_bytes {
            return Err(FileError32::Capacity);
        }
        if length != 0 {
            memory
                .check_span(GuestAddress(address), length as usize, Access::Read)
                .map_err(|_| FileError32::NoAccess)?;
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length as usize)
            .map_err(|_| FileError32::Allocation)?;
        bytes.resize(length as usize, 0);
        memory
            .read(GuestAddress(address), &mut bytes)
            .map_err(|_| FileError32::NoAccess)?;
        Ok(bytes)
    }
}
