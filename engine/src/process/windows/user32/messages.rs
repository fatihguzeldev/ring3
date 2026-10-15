use super::{
    User32CallContext32, User32Context32, User32Outcome32, callback::Continuation32, memory,
};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::{
        user32::{Message32, User32Api32, User32Error32},
        window::WindowHandle32,
    },
};

impl User32Context32 {
    pub(super) fn retrieve_message(
        &mut self,
        api: User32Api32,
        args: &[u32],
        memory: &mut AddressSpace,
        context: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        let blocking = matches!(api, User32Api32::GetMessageA | User32Api32::GetMessageW);
        if args[2] > 0xffff || args[3] > 0xffff || (args[3] != 0 && args[2] > args[3]) {
            return Err(User32Error32::Win32(87));
        }
        let remove = blocking || args[4] & 1 != 0;
        if !blocking && args[4] & !(3 | (0x1cff << 16)) != 0 {
            return Err(User32Error32::Win32(87));
        }
        let queue_mask = if blocking || args[4] >> 16 == 0 {
            u32::MAX
        } else {
            args[4] >> 16
        };
        let observed = (queue_mask | 8)
            & if args[2] == 0 && args[3] == 0 {
                u32::MAX
            } else {
                !0x100
            };
        if let Some((index, message)) = self.state.find_message_mask(
            context.thread_id,
            args[1],
            args[2],
            args[3],
            queue_mask,
        )? {
            memory::write_message(memory, args[0], message)?;
            self.state.observe_queue(context.thread_id, observed);
            self.input.message = message;
            if remove && let Some(keyboard) = self.state.remove_message(index) {
                self.input.queued = keyboard.state;
            }
            return Ok(User32Outcome32::Return(if blocking {
                u32::from(message.message != 0x0012)
            } else {
                1
            }));
        }
        self.state.observe_queue(context.thread_id, observed);
        if !blocking {
            return Ok(User32Outcome32::Return(0));
        }
        self.start(Continuation32::MessageWait {
            thread: context.thread_id,
            output: args[0],
            window: args[1],
            min: args[2],
            max: args[3],
        })
    }

    pub(crate) fn resume_wait(
        &mut self,
        token: u64,
        memory: &mut AddressSpace,
    ) -> Result<User32Outcome32, User32Error32> {
        let pending = *self
            .pending
            .last()
            .filter(|pending| pending.token == token)
            .ok_or(User32Error32::InvalidToken)?;
        let Continuation32::MessageWait {
            thread,
            output,
            window,
            min,
            max,
        } = pending.continuation
        else {
            return Err(User32Error32::InvalidToken);
        };
        let message = match self.state.find_message(thread, window, min, max) {
            Ok(message) => message,
            Err(User32Error32::Window(_) | User32Error32::Win32(1400)) => {
                self.last_error = Some(1400);
                self.pending.pop();
                return Ok(User32Outcome32::Return(u32::MAX));
            }
            Err(error) => return Err(error),
        };
        if let Some((index, message)) = message {
            memory::write_message(memory, output, message)?;
            self.state.observe_queue(
                thread,
                if min == 0 && max == 0 {
                    u32::MAX
                } else {
                    !0x100
                },
            );
            self.input.message = message;
            if let Some(keyboard) = self.state.remove_message(index) {
                self.input.queued = keyboard.state;
            }
            self.pending.pop();
            Ok(User32Outcome32::Return(u32::from(
                message.message != 0x0012,
            )))
        } else {
            Ok(User32Outcome32::Wait {
                continuation: token,
            })
        }
    }

    pub(super) fn dispatch_message(
        &mut self,
        address: u32,
        memory: &AddressSpace,
        context: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        let message = memory::message(memory, address)?;
        if message.message == 0x0113 && message.lparam != 0 {
            return self.start(Continuation32::Call {
                procedure: GuestAddress(message.lparam),
                arguments: [
                    message.window,
                    message.message,
                    message.wparam,
                    message.time_ms,
                ],
            });
        }
        if message.window == 0 {
            return Ok(User32Outcome32::Return(0));
        }
        let handle = self.owned(message.window, context.thread_id)?;
        let procedure = self.windows().get(handle)?.description.procedure;
        self.start(Continuation32::Call {
            procedure,
            arguments: [
                message.window,
                message.message,
                message.wparam,
                message.lparam,
            ],
        })
    }

    pub(super) fn post_message(
        &mut self,
        args: &[u32],
        context: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        if args[1] > 0xffff {
            return Err(User32Error32::Win32(87));
        }
        if args[0] == 0xffff || args[0] == u32::MAX {
            return Err(User32Error32::Win32(50));
        }
        let thread = if args[0] == 0 {
            context.thread_id
        } else {
            self.windows()
                .get(WindowHandle32::try_from(args[0])?)?
                .description
                .thread_id
        };
        if args[1] < 0x0400 && message_has_pointer(args[1]) {
            return Err(User32Error32::Win32(1159));
        }
        self.state.post(Message32 {
            window: args[0],
            message: args[1],
            wparam: args[2],
            lparam: args[3],
            time_ms: context.time_ms,
            point: context.cursor,
            thread_id: thread,
        })?;
        Ok(User32Outcome32::Return(1))
    }

    pub(super) fn post_quit(
        &mut self,
        code: u32,
        context: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        self.state.post_quit(Message32 {
            window: 0,
            message: 0x0012,
            wparam: code,
            lparam: 0,
            time_ms: context.time_ms,
            point: context.cursor,
            thread_id: context.thread_id,
        })?;
        Ok(User32Outcome32::Return(0))
    }

    pub(super) fn call_window_proc(
        &mut self,
        args: &[u32],
    ) -> Result<User32Outcome32, User32Error32> {
        if args[0] == 0 {
            return Err(User32Error32::Win32(87));
        }
        self.start(Continuation32::Call {
            procedure: GuestAddress(args[0]),
            arguments: [args[1], args[2], args[3], args[4]],
        })
    }

    pub(super) fn send_message(
        &mut self,
        args: &[u32],
        context: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        if args[0] == 0xffff {
            return Err(User32Error32::Win32(50));
        }
        let handle = self.owned(args[0], context.thread_id)?;
        self.start(Continuation32::Call {
            procedure: self.windows().get(handle)?.description.procedure,
            arguments: [args[0], args[1], args[2], args[3]],
        })
    }
}

fn message_has_pointer(message: u32) -> bool {
    matches!(
        message,
        0x0001
            | 0x000c
            | 0x000d
            | 0x0024
            | 0x002b
            | 0x002c
            | 0x002d
            | 0x0039
            | 0x0046
            | 0x0047
            | 0x004a
            | 0x007c
            | 0x007d
            | 0x0081
            | 0x0083
    )
}
