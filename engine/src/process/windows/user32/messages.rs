mod provenance;
mod waiting;
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
pub(super) use provenance::MessageIdentity32;
pub(super) use provenance::MessageProvenance32;

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
        let resume = super::activation::AppResume32::Retrieve {
            api,
            arguments: core::array::from_fn(|index| args.get(index).copied().unwrap_or(0)),
            context,
        };
        if let Some(outcome) = self.begin_app_notifications(resume)? {
            return Ok(outcome);
        }
        if let Some((index, message)) = self.state.find_message_mask(
            context.thread_id,
            args[1],
            args[2],
            args[3],
            queue_mask,
        )? {
            let (sequence, activation) = self.state.message_activation(index);
            let delivery = self.provenance.prepare(
                MessageIdentity32 {
                    timer: false,
                    sequence,
                },
                message,
                activation,
            )?;
            memory::write_message(memory, args[0], message)?;
            self.provenance.commit(delivery);
            self.observe_queue_with_timers(context.thread_id, observed);
            self.input.message = message;
            if remove && let Some(keyboard) = self.state.remove_message(index) {
                self.input.queued = keyboard.state;
            }
            if remove {
                self.raw_input.mark_delivered(
                    context.thread_id,
                    if message.message == 0xff {
                        message.lparam
                    } else {
                        0
                    },
                );
            }
            return Ok(User32Outcome32::Return(if blocking {
                u32::from(message.message != 0x0012)
            } else {
                1
            }));
        }
        if let Some(message) = self.state.paint_message(
            context.thread_id,
            args[1],
            args[2],
            args[3],
            queue_mask,
            context.time_ms,
            self.runtime_input.cursor,
        )? {
            memory::write_message(memory, args[0], message)?;
            self.observe_queue_with_timers(context.thread_id, observed);
            self.input.message = message;
            return Ok(User32Outcome32::Return(1));
        }
        if let Some(message) = self.retrieve_timer_message(
            memory,
            super::timers::TimerMessageQuery32 {
                output: args[0],
                thread: context.thread_id,
                window: args[1],
                min: args[2],
                max: args[3],
                remove,
                mask: queue_mask,
            },
        )? {
            self.observe_queue_with_timers(context.thread_id, observed);
            self.input.message = message;
            return Ok(User32Outcome32::Return(1));
        }
        self.observe_queue_with_timers(context.thread_id, observed);
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
        if let Some(outcome) = self.resume_file_prompt_dialog(token, memory) {
            return outcome;
        }
        let outcome = self.resume_wait_step(token, memory)?;
        self.complete_user32_child(token, outcome, memory)
    }

    fn resume_wait_step(
        &mut self,
        token: u64,
        memory: &mut AddressSpace,
    ) -> Result<User32Outcome32, User32Error32> {
        let pending = self
            .pending
            .last()
            .filter(|pending| pending.token == token)
            .cloned()
            .ok_or(User32Error32::InvalidToken)?;
        if let Continuation32::Animation(_) = pending.continuation {
            return self.resume_animation(token, memory);
        }
        if let Continuation32::Numeric(_) = pending.continuation {
            return self.resume_numeric(token, memory);
        }
        if let Continuation32::ListView(_) = pending.continuation {
            return self.resume_listview_call(token, memory);
        }
        if let Continuation32::Header(_) = pending.continuation {
            return self.resume_header_call(token, memory);
        }
        if let Continuation32::TabSelection(_) = pending.continuation {
            return self.resume_tab_selection(token, memory);
        }
        if let Continuation32::CommonLayout(_) = pending.continuation {
            return self.resume_common_layout(token);
        }
        if let Continuation32::UiState(_) = pending.continuation {
            return self.resume_ui_change(token);
        }
        if let Continuation32::SystemColors(_) = pending.continuation {
            return self.resume_system_colors(token);
        }
        if let Continuation32::SentPaint(_) = pending.continuation {
            return self.resume_sent_paint(token, memory);
        }
        if let Continuation32::Clipboard(_) = pending.continuation {
            return self.resume_clipboard(token);
        }
        if let Continuation32::Accelerator(_) = pending.continuation {
            return self.resume_accelerator(token);
        }
        if let Continuation32::ResourceDialog { plan, .. } = pending.continuation {
            return self.resume_resource_dialog(plan, memory);
        }
        if let Continuation32::Dialog { id } = pending.continuation {
            return self.resume_dialog(id, memory);
        }
        if let Continuation32::DialogRemoteHelp { id, scratch, ready } = pending.continuation {
            return self.resume_remote_dialog_help(id, scratch, ready, memory);
        }
        if let Continuation32::Retrieve {
            api,
            arguments,
            thread,
        } = pending.continuation
        {
            self.pending.pop();
            let outcome = self.prepare_arguments(
                api,
                &arguments[..api.stack_words() as usize],
                memory,
                User32CallContext32 {
                    thread_id: thread,
                    ..Default::default()
                },
            );
            if outcome.is_err() {
                self.pending.push(pending);
            }
            return outcome;
        }
        if let Continuation32::RemoteCall { ready, .. } = pending.continuation {
            return if let Some(value) = ready {
                self.pending.pop();
                Ok(User32Outcome32::Return(value))
            } else {
                Ok(User32Outcome32::Wait {
                    continuation: token,
                })
            };
        }
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
        if self.app_activation.pending(thread) {
            self.pending.pop();
            let outcome = self.retrieve_message(
                User32Api32::GetMessageW,
                &[output, window, min, max],
                memory,
                User32CallContext32 {
                    thread_id: thread,
                    ..self.runtime_input
                },
            );
            if outcome.is_err() {
                self.pending.push(pending);
            }
            return outcome;
        }
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
            let (sequence, activation) = self.state.message_activation(index);
            let delivery = self.provenance.prepare(
                MessageIdentity32 {
                    timer: false,
                    sequence,
                },
                message,
                activation,
            )?;
            memory::write_message(memory, output, message)?;
            self.provenance.commit(delivery);
            self.observe_queue_with_timers(
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
            self.raw_input.mark_delivered(
                thread,
                if message.message == 0xff {
                    message.lparam
                } else {
                    0
                },
            );
            self.pending.pop();
            Ok(User32Outcome32::Return(u32::from(
                message.message != 0x0012,
            )))
        } else if let Some(message) = self.state.paint_message(
            thread,
            window,
            min,
            max,
            u32::MAX,
            self.runtime_input.time_ms,
            self.runtime_input.cursor,
        )? {
            memory::write_message(memory, output, message)?;
            self.observe_queue_with_timers(thread, u32::MAX);
            self.input.message = message;
            self.pending.pop();
            Ok(User32Outcome32::Return(1))
        } else if let Some(message) = self.retrieve_timer_message(
            memory,
            super::timers::TimerMessageQuery32 {
                output,
                thread,
                window,
                min,
                max,
                remove: true,
                mask: u32::MAX,
            },
        )? {
            self.observe_queue_with_timers(thread, u32::MAX);
            self.input.message = message;
            self.pending.pop();
            Ok(User32Outcome32::Return(1))
        } else {
            Ok(User32Outcome32::Wait {
                continuation: token,
            })
        }
    }

    pub(in crate::process::windows::user32) fn check_message_dispatch(
        &self,
        address: u32,
        memory: &AddressSpace,
        context: User32CallContext32,
    ) -> Result<Message32, User32Error32> {
        let message = memory::message(memory, address)?;
        if message.message == 0x0113 && message.lparam != 0 {
            self.check_continuation()?;
        } else if message.window != 0 {
            self.owned(message.window, context.thread_id)?;
            self.check_continuation()?;
        }
        Ok(message)
    }

    pub(super) fn dispatch_message(
        &mut self,
        address: u32,
        memory: &AddressSpace,
        context: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        let message = self.check_message_dispatch(address, memory, context)?;
        if message.message == 0x0113 && message.lparam != 0 {
            return self.start_message_callback(
                Continuation32::Timer {
                    procedure: GuestAddress(message.lparam),
                    arguments: [
                        message.window,
                        message.message,
                        message.wparam,
                        context.time_ms,
                    ],
                },
                message,
                context.thread_id,
            );
        }
        if message.window == 0 {
            return Ok(User32Outcome32::Return(0));
        }
        let handle = self.owned(message.window, context.thread_id)?;
        let procedure = self.windows().get(handle)?.description.procedure;
        self.start_message_callback(
            Continuation32::Call {
                procedure,
                arguments: [
                    message.window,
                    message.message,
                    message.wparam,
                    message.lparam,
                ],
            },
            message,
            context.thread_id,
        )
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
        self.state.post_captured(
            Message32 {
                window: args[0],
                message: args[1],
                wparam: args[2],
                lparam: args[3],
                time_ms: context.time_ms,
                point: context.cursor,
                thread_id: thread,
            },
            self.call_activation.clone(),
        )?;
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
        if args[1] > 0xffff {
            return Err(User32Error32::Win32(87));
        }
        let handle = WindowHandle32::try_from(args[0])?;
        let record = self.windows().get(handle)?;
        if record.description.thread_id != context.thread_id {
            return self.start(Continuation32::RemoteCall { ready: None });
        }
        self.start(Continuation32::Call {
            procedure: record.description.procedure,
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
