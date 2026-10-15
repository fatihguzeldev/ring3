use super::{
    User32CallContext32, User32Context32, User32Outcome32,
    callback::{Continuation32, Pending32},
    raw,
};
use crate::{
    memory::{AddressSpace, PageRange},
    windows::{
        CallFrame32,
        user32::{User32Api32, User32Error32, User32Limits32, User32State32},
        window::{WindowContext32, WindowError32, WindowHandle32},
    },
};

impl User32Context32 {
    pub(crate) fn new(
        limits: User32Limits32,
        scratch_bounds: PageRange,
    ) -> Result<Self, User32Error32> {
        if scratch_bounds.first == 0 {
            return Err(User32Error32::InvalidLimits);
        }
        let state = User32State32::new(limits)?;
        let mut pending = Vec::new();
        pending
            .try_reserve_exact(limits.continuations)
            .map_err(|_| User32Error32::Allocation)?;
        let mut destructions = Vec::new();
        destructions
            .try_reserve_exact(limits.continuations)
            .map_err(|_| User32Error32::Allocation)?;
        Ok(Self {
            state,
            scratch_bounds,
            pending,
            continuation_limit: limits.continuations,
            next_token: 0,
            last_error: None,
            runtime_input: User32CallContext32::default(),
            input: super::input::User32Input32::default(),
            destructions,
        })
    }

    pub(crate) fn windows(&self) -> &WindowContext32 {
        self.state.windows()
    }
    pub(in crate::process) fn is_top_level_window(&self, raw: u32) -> bool {
        WindowHandle32::try_from(raw)
            .ok()
            .and_then(|handle| self.state.record(handle).ok())
            .is_some_and(|data| {
                !data.destroying
                    && data.style & 0x4000_0000 == 0
                    && self
                        .windows()
                        .get(data.handle)
                        .is_ok_and(|record| record.description.thread_id == 1)
            })
    }
    pub(in crate::process) fn source_cursor(&self) -> Option<crate::input::source::InputCursor32> {
        self.input.cursor
    }
    pub(crate) fn take_last_error(&mut self) -> Option<u32> {
        self.last_error.take()
    }
    pub(in crate::process) fn wait_queue_status(&self) -> (u32, u32, u64) {
        self.state.queue_status(1)
    }

    pub(crate) fn prepare(
        &mut self,
        api: User32Api32,
        frame: &CallFrame32,
        memory: &mut AddressSpace,
        context: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        if frame.convention() != api.convention()
            || frame.stack_words() != api.stack_words()
            || context.thread_id == 0
        {
            return Err(User32Error32::InvalidRequest);
        }
        self.prepare_arguments(api, frame.arguments(), memory, context)
    }

    pub(super) fn prepare_arguments(
        &mut self,
        api: User32Api32,
        args: &[u32],
        memory: &mut AddressSpace,
        context: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        self.last_error = None;
        if args.len() != api.stack_words() as usize {
            return Err(User32Error32::InvalidRequest);
        }
        use User32Api32::*;
        let result = match api {
            RegisterClassA | RegisterClassW | RegisterClassExA | RegisterClassExW => {
                self.register_class(api, args[0], memory, context)
            }
            UnregisterClassA | UnregisterClassW => {
                self.unregister_class(api, args, memory, context)
            }
            CreateWindowExA | CreateWindowExW => self.create_window(api, args, memory, context),
            DestroyWindow => self.destroy_window(args[0], context),
            GetClientRect | GetWindowRect => self.get_rect(api, args, memory),
            IsWindow => Ok(User32Outcome32::Return(u32::from(
                WindowHandle32::try_from(args[0])
                    .ok()
                    .is_some_and(|handle| self.windows().get(handle).is_ok()),
            ))),
            GetFocus => Ok(User32Outcome32::Return(raw(self.windows().focus()))),
            GetCapture => Ok(User32Outcome32::Return(raw(self.windows().capture()))),
            SetFocus | SetCapture | ReleaseCapture => self.set_focus_capture(api, args, context),
            ShowWindow => self.show_window(args, context),
            MoveWindow | SetWindowPos => self.move_window(api, args, memory, context),
            PeekMessageA | PeekMessageW | GetMessageA | GetMessageW => {
                self.retrieve_message(api, args, memory, context)
            }
            TranslateMessage => self.translate_message(args[0], memory, context),
            DispatchMessageA | DispatchMessageW => self.dispatch_message(args[0], memory, context),
            PostMessageA | PostMessageW => self.post_message(args, context),
            PostQuitMessage => self.post_quit(args[0], context),
            DefWindowProcA | DefWindowProcW => self.def_window_proc(args, memory, context),
            GetWindowLongA | GetWindowLongW => self.get_window_long(args),
            SetWindowLongA | SetWindowLongW => self.set_window_long(args, memory, context),
            CallWindowProcA | CallWindowProcW => self.call_window_proc(args),
            SendMessageA | SendMessageW => self.send_message(args, context),
            MsgWaitForMultipleObjects | MsgWaitForMultipleObjectsEx | WaitMessage => {
                Err(User32Error32::InvalidRequest)
            }
            _ => self.input_query(api, args, memory, context),
        };
        match result {
            Err(User32Error32::Win32(error)) => {
                self.last_error = Some(error);
                Ok(User32Outcome32::Return(
                    if matches!(api, GetMessageA | GetMessageW) {
                        u32::MAX
                    } else {
                        0
                    },
                ))
            }
            Err(User32Error32::Window(error)) => {
                self.last_error = Some(match error {
                    WindowError32::InvalidHandle => 1400,
                    WindowError32::InvalidThread => 5,
                    WindowError32::Disabled => 87,
                    WindowError32::Capacity
                    | WindowError32::QueueFull
                    | WindowError32::Exhausted => 8,
                    _ => 87,
                });
                Ok(User32Outcome32::Return(
                    if matches!(api, GetMessageA | GetMessageW) {
                        u32::MAX
                    } else {
                        0
                    },
                ))
            }
            Err(User32Error32::Capacity) => {
                self.last_error = Some(8);
                Ok(User32Outcome32::Return(0))
            }
            result => result,
        }
    }

    pub(super) fn owned(
        &self,
        raw_handle: u32,
        thread_id: u32,
    ) -> Result<WindowHandle32, User32Error32> {
        let handle = WindowHandle32::try_from(raw_handle)?;
        if self.windows().get(handle)?.description.thread_id != thread_id {
            return Err(User32Error32::Win32(5));
        }
        Ok(handle)
    }

    pub(super) fn check_continuation(&self) -> Result<u64, User32Error32> {
        if self.pending.len() == self.continuation_limit {
            return Err(User32Error32::Capacity);
        }
        self.next_token
            .checked_add(1)
            .ok_or(User32Error32::Capacity)
    }

    pub(super) fn start(
        &mut self,
        continuation: Continuation32,
    ) -> Result<User32Outcome32, User32Error32> {
        let token = self.check_continuation()?;
        self.next_token = token;
        self.pending.push(Pending32 {
            token,
            continuation,
        });
        self.next_outcome()
    }
}
