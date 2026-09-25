use super::{
    Cpu32, DispatchError, GuestMemory, MemoryError, Process32, ProcessStop, Register32, thread,
};

/// a snapshotted `MB_OK` request; strings contain ansi bytes without trailing nul.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MessageBoxRequest<'a> {
    pub id: u64,
    pub owner: u32,
    pub text: &'a [u8],
    pub caption: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageBoxResponseError {
    Exited,
    NoRequest,
    StaleRequest,
    AlreadyAcknowledged,
}

struct Pending {
    id: u64,
    frame: [u32; 5],
    cpu: Cpu32,
    text: Vec<u8>,
    caption: Vec<u8>,
    acknowledged: bool,
}

#[derive(Default)]
pub(super) struct State {
    last_id: u64,
    pending: Option<Pending>,
}

impl State {
    pub(super) fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub(super) fn stop(&self, cpu: &Cpu32) -> Option<ProcessStop> {
        let pending = self.pending.as_ref()?;
        if pending.cpu != *cpu {
            return Some(ProcessStop::UnsupportedApi { address: cpu.eip });
        }
        (!pending.acknowledged).then_some(ProcessStop::MessageBoxRequired)
    }
}

impl Process32 {
    /// borrows a pending message that the host must display before acknowledging.
    /// the whole guest pauses on positive run budgets until the host responds;
    /// this bounded `MB_OK` profile does not run a reentrant guest modal message loop.
    #[must_use]
    pub fn pending_message_box(&self) -> Option<MessageBoxRequest<'_>> {
        let pending = self.message_box.pending.as_ref()?;
        (!pending.acknowledged).then_some(MessageBoxRequest {
            id: pending.id,
            owner: pending.frame[1],
            text: &pending.text,
            caption: &pending.caption,
        })
    }

    /// records an explicit host acknowledgement of the displayed `MB_OK` request.
    /// does not advance execution, change the clock or mutate guest state.
    /// the next positive run completes the original call with `IDOK` (1).
    /// dropping the process cancels any pending request.
    ///
    /// # errors
    /// rejects an exited process, missing/stale request or duplicate acknowledgement.
    #[expect(clippy::missing_errors_doc, reason = "project headings are lower case")]
    pub fn acknowledge_message_box(&mut self, id: u64) -> Result<(), MessageBoxResponseError> {
        if self.exit_code.is_some() {
            return Err(MessageBoxResponseError::Exited);
        }
        let pending = self
            .message_box
            .pending
            .as_mut()
            .ok_or(MessageBoxResponseError::NoRequest)?;
        if pending.id != id {
            return Err(MessageBoxResponseError::StaleRequest);
        }
        if pending.acknowledged {
            return Err(MessageBoxResponseError::AlreadyAcknowledged);
        }
        pending.acknowledged = true;
        Ok(())
    }

    pub(super) fn message_box(&mut self, frame: &[u32]) -> Result<bool, DispatchError> {
        if let Some(pending) = &self.message_box.pending {
            if pending.cpu != self.cpu || pending.frame != frame {
                return Err(DispatchError::Unsupported);
            }
            if !pending.acknowledged {
                return Err(DispatchError::MessageBoxRequired);
            }
            self.message_box.pending = None;
            self.cpu.set_register(Register32::Eax, 1);
            return Ok(false);
        }
        if frame[4] != 0 {
            return Err(DispatchError::Unsupported);
        }
        if frame[1] != 0 {
            let Some(owner) = self.desktop.window(frame[1]) else {
                thread::Teb(self.cpu.fs_base()).set_last_error(&mut self.memory, 1400)?;
                self.cpu.set_register(Register32::Eax, 0);
                return Ok(false);
            };
            if owner.parent != 0 {
                return Err(DispatchError::Unsupported);
            }
        }
        let text = read_string(&self.memory, frame[2], b"")?;
        let caption = read_string(&self.memory, frame[3], b"Error")?;
        let id = self
            .message_box
            .last_id
            .checked_add(1)
            .ok_or(DispatchError::Unsupported)?;
        self.message_box.pending = Some(Pending {
            id,
            frame: frame.try_into().expect("message box frame has five words"),
            cpu: self.cpu,
            text,
            caption,
            acknowledged: false,
        });
        self.message_box.last_id = id;
        Err(DispatchError::MessageBoxRequired)
    }
}

fn read_string(
    memory: &GuestMemory,
    address: u32,
    default: &[u8],
) -> Result<Vec<u8>, DispatchError> {
    if address == 0 {
        return Ok(default.to_vec());
    }
    let mut bytes = Vec::new();
    for offset in 0..4096 {
        let source = address
            .checked_add(offset)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut byte = [0];
        memory.read(u64::from(source), &mut byte)?;
        if byte[0] == 0 {
            return Ok(bytes);
        }
        bytes.push(byte[0]);
    }
    Err(DispatchError::Unsupported)
}
