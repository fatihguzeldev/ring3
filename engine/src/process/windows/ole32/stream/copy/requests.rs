use super::context::{CHUNK, CopyPhase32, MappingWitness32, StreamCopy32};
use crate::{
    memory::{Access, GuestAddress, PAGE_SIZE},
    process::{
        EngineInstance, HostError,
        calls::call::PendingOwner,
        windows::{
            callbacks::GuestCallbackScratch32,
            ole32::{
                activation::ActivationRuntime32,
                marshaling::{ExecutionApartment32, Remote32},
            },
            threads::ThreadInvocationCompletion32,
        },
    },
    windows::{CallFrame32, ole32::stream::StreamId32},
};
impl EngineInstance {
    pub(in crate::process::windows::ole32) fn begin_stream_copy(
        &mut self,
        owner: PendingOwner,
        token: u32,
        frame: CallFrame32,
        id: StreamId32,
    ) -> Result<(), HostError> {
        let args = frame.arguments();
        let thread = self.current_windows_thread_id()?;
        let foreign = (|| {
            if args[1] == 0 {
                return Err(HostError::InvalidRequest);
            }
            let table = self.ole32_read_word(args[1])?;
            let slot = table.checked_add(16).ok_or(HostError::InvalidRequest)?;
            let procedure = self.com_guest_method(args[1], 4)?;
            let witness = MappingWitness32::capture(
                self.memory()?,
                &[
                    (args[1], 4, Access::Read),
                    (slot, 4, Access::Read),
                    (procedure.0, 1, Access::Execute),
                    (args[4], 8, Access::Write),
                    (args[5], 8, Access::Write),
                ],
            )
            .map_err(HostError::Memory)?;
            Ok((table, procedure, witness))
        })();
        let (table, procedure, witness) = match foreign {
            Ok(value) => value,
            Err(_) => {
                self.publish_prepared_call(Self::prepare_completed_call(
                    frame.complete(0x8003_0009),
                )?);
                return Ok(());
            }
        };
        let runtime = self.ole32.as_ref().unwrap();
        let execution = ExecutionApartment32 {
            thread,
            apartment: runtime
                .apartments
                .identity(thread)
                .map_err(|_| HostError::InvalidRequest)?,
        };
        if runtime.streams.copies.len() >= runtime.streams.copy_limit {
            return Err(HostError::Infrastructure);
        }
        let continuation = runtime
            .activation
            .next
            .checked_add(1)
            .ok_or(HostError::Infrastructure)?;
        let requested = u64::from(args[2]) | (u64::from(args[3]) << 32);
        let read = runtime
            .streams
            .store
            .prepare_read(id, requested.min(CHUNK as u64) as usize)
            .map_err(|_| HostError::Infrastructure)?;
        let count = read.length();
        let range = self
            .memory()?
            .find_free_range(ActivationRuntime32::scratch_reservation(), 1, PAGE_SIZE)
            .map_err(HostError::Memory)?;
        let address = GuestAddress(range.first as u32 * PAGE_SIZE);
        let invocation = if count != 0 {
            Some(self.prepare_thread_invocation(
                thread,
                procedure,
                &[args[1], address.0, count as u32, address.0 + CHUNK as u32],
                ThreadInvocationCompletion32::Ole32 {
                    sender: thread,
                    continuation,
                },
            )?)
        } else {
            None
        };
        let runtime = self.ole32.as_ref().unwrap();
        let hold = runtime
            .com
            .prepare_lifetime_changes(self.memory()?, &[], &[], &[GuestAddress(args[0])], &[], &[])
            .map_err(super::super::super::rpc::com_reference_error)?;
        runtime
            .com
            .can_commit_lifetime_changes(self.memory()?, &hold)
            .map_err(super::super::super::rpc::com_reference_error)?;
        let mut bytes = [0u8; PAGE_SIZE as usize];
        bytes[..count].copy_from_slice(
            runtime
                .streams
                .store
                .read_bytes(&read)
                .map_err(|_| HostError::Infrastructure)?,
        );
        let scratch = GuestCallbackScratch32::allocate(
            self.memory.as_mut().unwrap(),
            ActivationRuntime32::scratch_reservation(),
            &bytes,
        )
        .map_err(HostError::Memory)?;
        assert_eq!(scratch.address, address);
        let runtime = self.ole32.as_mut().unwrap();
        runtime.com.commit_lifetime_after_aggregate_unmap(
            self.memory.as_ref().unwrap(),
            hold,
            true,
        );
        if count != 0 {
            runtime.streams.store.commit_read(read);
        }
        runtime.activation.next = continuation;
        runtime.streams.copies.push(StreamCopy32 {
            continuation,
            owner,
            token,
            frame,
            execution,
            source: GuestAddress(args[0]),
            id,
            destination: args[1],
            table,
            procedure,
            witness,
            scratch,
            remaining: requested - count as u64,
            read: count as u64,
            written: 0,
            chunk: count as u32,
            remote: if invocation.is_some() {
                Remote32::Queued
            } else {
                Remote32::Idle
            },
            phase: if invocation.is_some() {
                CopyPhase32::Write
            } else {
                CopyPhase32::Finish(0)
            },
        });
        if let Some(invocation) = invocation {
            self.commit_thread_invocation(invocation);
            self.wake_com_apartment_sender(thread);
            Ok(())
        } else {
            self.advance_stream_copy(self.ole32.as_ref().unwrap().streams.copies.len() - 1)
        }
    }
}
