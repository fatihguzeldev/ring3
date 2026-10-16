use crate::{
    memory::{AddressSpace, GuestAddress},
    process::windows::user32::{
        User32Callback32, User32Context32, User32Outcome32, callback::Continuation32,
        memory::Scratch32,
    },
    windows::{user32::User32Error32 as E, window::WindowHandle32},
};
impl User32Context32 {
    pub(in crate::process::windows::user32) fn resume_dialog(
        &mut self,
        id: u64,
        memory: &mut AddressSpace,
    ) -> Result<User32Outcome32, E> {
        let Some(index) = self.dialogs.iter().position(|dialog| dialog.id == id) else {
            self.pending.pop();
            self.last_error = Some(1400);
            return Ok(User32Outcome32::Return(0));
        };
        let dialog = &self.dialogs[index];
        if let Some((result, error)) = dialog.result {
            self.dialogs.swap_remove(index);
            self.pending.pop();
            if error != 0 {
                self.last_error = Some(error);
            }
            return Ok(User32Outcome32::Return(result));
        }
        if !dialog.help {
            return Ok(User32Outcome32::Wait {
                continuation: self.pending.last().unwrap().token,
            });
        }
        let mut bytes = [0; 28];
        for (index, word) in [
            28,
            1,
            0,
            dialog.owner,
            dialog.help_context,
            self.input.query_point.x as u32,
            self.input.query_point.y as u32,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        let (procedure, count) = if dialog.help_callback != 0 {
            (dialog.help_callback, 1)
        } else if dialog.owner != 0 {
            let window = self
                .windows()
                .get(WindowHandle32::try_from(dialog.owner)?)?;
            if window.description.thread_id != dialog.thread {
                return Err(E::Win32(120));
            }
            (window.description.procedure.0, 4)
        } else {
            self.dialogs[index].help = false;
            return Ok(User32Outcome32::Wait {
                continuation: self.pending.last().unwrap().token,
            });
        };
        let owner = dialog.owner;
        let scratch = Scratch32::allocate(memory, self.scratch_bounds, &bytes)?;
        self.dialogs[index].help = false;
        let pending = self.pending.last_mut().unwrap();
        pending.continuation = Continuation32::DialogHelp { id, scratch };
        Ok(User32Outcome32::Callback(User32Callback32 {
            procedure: GuestAddress(procedure),
            arguments: if count == 1 {
                [scratch.address.0, 0, 0, 0]
            } else {
                [owner, 0x53, 0, scratch.address.0]
            },
            count,
            continuation: pending.token,
        }))
    }
    pub(in crate::process::windows::user32) fn complete_dialog_help(
        &mut self,
        id: u64,
        scratch: Scratch32,
        memory: &mut AddressSpace,
    ) -> Result<User32Outcome32, E> {
        scratch.release(memory)?;
        self.pending.last_mut().unwrap().continuation = Continuation32::Dialog { id };
        self.next_outcome()
    }
}
