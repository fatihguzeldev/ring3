use crate::{
    memory::{GuestAddress, PAGE_SIZE},
    process::{
        EngineInstance, HostError,
        windows::{
            threads::ThreadInvocationCompletion32,
            user32::{User32Outcome32, callback::Continuation32, memory::Scratch32},
        },
    },
    windows::{user32::User32Error32 as E, window::WindowHandle32},
};
impl EngineInstance {
    pub(in crate::process) fn dispatch_user32_dialog_remote_help(
        &mut self,
        continuation: u64,
    ) -> Result<(), HostError> {
        let context = self.user32.as_ref().ok_or(HostError::InvalidRequest)?;
        let Some(pending) = context
            .pending
            .last()
            .filter(|pending| pending.token == continuation)
        else {
            return Ok(());
        };
        let Continuation32::Dialog { id } = pending.continuation else {
            return Ok(());
        };
        let Some(dialog) = context.dialogs.iter().find(|dialog| {
            dialog.id == id
                && dialog.help
                && dialog.result.is_none()
                && dialog.help_callback == 0
                && dialog.owner != 0
        }) else {
            return Ok(());
        };
        let window = context
            .windows()
            .get(
                WindowHandle32::try_from(dialog.owner)
                    .map_err(|error| HostError::User32(error.into()))?,
            )
            .map_err(|error| HostError::User32(error.into()))?;
        if window.description.thread_id == dialog.thread {
            return Ok(());
        }
        let mut bytes = [0; 28];
        for (index, value) in [
            28,
            1,
            0,
            dialog.owner,
            dialog.help_context,
            context.input.query_point.x as u32,
            context.input.query_point.y as u32,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        let bounds = context.scratch_bounds;
        let range = self
            .memory()?
            .find_free_range(bounds, 1, PAGE_SIZE)
            .map_err(HostError::Memory)?;
        let address = range.first as u32 * PAGE_SIZE;
        let invocation = self.prepare_thread_invocation(
            window.description.thread_id,
            window.description.procedure,
            &[dialog.owner, 0x53, 0, address],
            ThreadInvocationCompletion32::User32 {
                sender: dialog.thread,
                continuation,
            },
        )?;
        let scratch = Scratch32::allocate(self.memory.as_mut().unwrap(), bounds, &bytes)
            .map_err(HostError::User32)?;
        assert_eq!(scratch.address, GuestAddress(address));
        let context = self.user32.as_mut().unwrap();
        context
            .dialogs
            .iter_mut()
            .find(|dialog| dialog.id == id)
            .unwrap()
            .help = false;
        context.pending.last_mut().unwrap().continuation = Continuation32::DialogRemoteHelp {
            id,
            scratch,
            ready: None,
        };
        self.commit_thread_invocation(invocation);
        Ok(())
    }
}
impl super::super::User32Context32 {
    pub(in crate::process::windows::user32) fn resume_remote_dialog_help(
        &mut self,
        id: u64,
        scratch: Scratch32,
        ready: Option<u32>,
        memory: &mut crate::memory::AddressSpace,
    ) -> Result<User32Outcome32, E> {
        if ready.is_none() {
            return Ok(User32Outcome32::Wait {
                continuation: self.pending.last().unwrap().token,
            });
        }
        scratch.release(memory)?;
        self.pending.last_mut().unwrap().continuation = Continuation32::Dialog { id };
        self.next_outcome()
    }
}
