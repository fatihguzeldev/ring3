use super::{
    User32CallContext32, User32Context32, User32Outcome32,
    callback::{Continuation32, Pending32},
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
            hooks: Default::default(),
            compatibility: Default::default(),
            menus: crate::windows::user32::menus::MenuStore32::new(Default::default())?,
            menu_popups: super::menus::PopupContext32::new(limits.continuations as usize)?,
            clipboard: Default::default(),
            accelerators: crate::windows::user32::accelerators::AcceleratorStore32::new(256, 4096)?,
            builtin_procedures: None,
            common_procedures: None,
            layouts: Vec::new(),
            process_layout: None,
            scratch_bounds,
            pending,
            continuation_limit: limits.continuations,
            next_token: 0,
            last_error: None,
            runtime_input: User32CallContext32::default(),
            input: super::input::User32Input32::default(),
            app_activation: super::activation::AppActivation32::new(limits.messages as usize)?,
            images: Default::default(),
            ime: crate::windows::imm32::InputContexts32::new(
                256,
                limits.windows,
                crate::windows::imm32::MAX_COMPOSITION_UNITS,
            )
            .map_err(|_| User32Error32::InvalidLimits)?,
            raw_input: crate::windows::rawinput::RawInputRuntime32::new(limits.messages)?,
            destructions,
            timers: super::timers::TimerState32::new(limits.messages, limits.windows)?,
            dialogs: Vec::new(),
            resource_dialogs: super::dialogs::resources::ResourceDialogs32::new(
                limits.continuations,
                limits.windows,
            )?,
            file_dialogs: super::file_dialogs::FileDialogs32::new(limits.continuations)?,
            provenance: super::messages::MessageProvenance32::new(limits.messages)?,
            call_activation: Default::default(),
            activation_capture_limit: limits
                .messages
                .checked_mul(5)
                .and_then(|count| count.checked_add(limits.continuations + 1))
                .ok_or(User32Error32::InvalidLimits)?,
        })
    }

    pub(in crate::process) fn activation_capture_limit(&self) -> usize {
        self.activation_capture_limit
    }

    pub(in crate::process) fn scratch_reservation(&self) -> PageRange {
        self.scratch_bounds
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
                    && self.windows().get(data.handle).is_ok()
            })
    }
    pub(in crate::process) fn source_cursor(&self) -> Option<crate::input::source::InputCursor32> {
        self.input.cursor
    }
    pub(crate) fn take_last_error(&mut self) -> Option<u32> {
        self.last_error.take()
    }
    pub(in crate::process) fn wait_queue_status(&self, thread: u32) -> (u32, u32, u64) {
        self.queue_status_with_timers(thread, self.timers.now)
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
        if api == User32Api32::WsprintfA {
            let arguments = frame.state().registers[crate::cpu::x86::Register32::Esp.index()]
                .checked_add(12)
                .ok_or(User32Error32::Win32(87))?;
            return self.format_user_string(
                frame.arguments()[0],
                frame.arguments()[1],
                arguments,
                memory,
                context.ansi,
            );
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
        self.prepare_thread_slots()?;
        self.prepare_thread_layout(context.thread_id)?;
        self.runtime_input.thread_id = context.thread_id;
        self.runtime_input.ansi = context.ansi;
        use User32Api32::*;
        let result = match api {
            RegisterWindowMessageA | RegisterWindowMessageW => {
                let name = super::memory::string(memory, args[0], api.wide(), 32767, context.ansi)?;
                self.state
                    .register_message(name)
                    .map(User32Outcome32::Return)
            }
            RegisterClassA | RegisterClassW | RegisterClassExA | RegisterClassExW => {
                self.register_class(api, args[0], memory, context)
            }
            UnregisterClassA | UnregisterClassW => {
                self.unregister_class(api, args, memory, context)
            }
            CreateWindowExA | CreateWindowExW => self.create_window(api, args, memory, context),
            DefDlgProcA | DefDlgProcW => self.def_dialog_proc(args, api.wide(), memory, context),
            EndDialog
            | GetDlgItem
            | GetDlgCtrlID
            | MapDialogRect
            | GetDialogBaseUnits
            | GetWindowContextHelpId
            | SetWindowContextHelpId => self.resource_dialog_query(
                api,
                args,
                memory,
                context,
                self.resource_dialogs.system_base,
            ),
            TrackPopupMenu | TrackPopupMenuEx => self.begin_popup_menu(api, args, memory, context),
            EndMenu => self.end_popup_menu(context.thread_id),
            CreateMenu | CreatePopupMenu | DestroyMenu | IsMenu | GetMenu | SetMenu
            | DrawMenuBar | GetMenuItemRect | GetMenuItemCount | GetMenuItemID | GetSubMenu
            | AppendMenuA | AppendMenuW | InsertMenuA | InsertMenuW | ModifyMenuA | ModifyMenuW
            | RemoveMenu | DeleteMenu | EnableMenuItem | CheckMenuItem | GetMenuState
            | GetMenuStringA | GetMenuStringW | GetMenuContextHelpId | SetMenuContextHelpId
            | GetMenuItemInfoA | GetMenuItemInfoW | SetMenuItemInfoA | SetMenuItemInfoW
            | InsertMenuItemA | InsertMenuItemW => self.menu_call(api, args, memory, context),
            CreateAcceleratorTableA
            | CreateAcceleratorTableW
            | CopyAcceleratorTableA
            | CopyAcceleratorTableW
            | DestroyAcceleratorTable
            | TranslateAcceleratorA
            | TranslateAcceleratorW => self.accelerator_call(api, args, memory, context),
            DestroyWindow => self.destroy_window(args[0], context),
            GetClientRect | GetWindowRect => self.get_rect(api, args, memory),
            IsWindow => Ok(User32Outcome32::Return(u32::from(
                WindowHandle32::try_from(args[0])
                    .ok()
                    .is_some_and(|handle| self.windows().get(handle).is_ok()),
            ))),
            GetFocus => Ok(User32Outcome32::Return(
                self.owned_input_window(self.windows().focus(), context.thread_id),
            )),
            GetCapture => Ok(User32Outcome32::Return(
                self.owned_input_window(self.windows().capture(), context.thread_id),
            )),
            SetFocus | SetCapture | ReleaseCapture => self.set_focus_capture(api, args, context),
            ShowWindow => self.show_window(args, context),
            MoveWindow | SetWindowPos => self.move_window(api, args, memory, context),
            PeekMessageA | PeekMessageW | GetMessageA | GetMessageW => {
                self.retrieve_message(api, args, memory, context)
            }
            SetTimer => self.set_timer(args, context),
            KillTimer => self.kill_timer(args, context),
            TranslateMessage => self.translate_message(args[0], memory, context),
            DispatchMessageA | DispatchMessageW => self.dispatch_message(args[0], memory, context),
            PostMessageA | PostMessageW => self.post_message(args, context),
            PostQuitMessage => self.post_quit(args[0], context),
            DefWindowProcA | DefWindowProcW => {
                self.def_window_proc_encoded(args, api.wide(), memory, context)
            }
            GetWindowLongA | GetWindowLongW => self.get_window_long(args),
            SetWindowLongA | SetWindowLongW => self.set_window_long(args, memory, context),
            CallWindowProcA | CallWindowProcW => self.call_window_proc(args),
            SendMessageA | SendMessageW | GetWindowTextA | GetWindowTextW
            | GetWindowTextLengthA | GetWindowTextLengthW | SetWindowTextA | SetWindowTextW => {
                self.send_message(&api.sent_message(args).unwrap(), context)
            }
            MsgWaitForMultipleObjects | MsgWaitForMultipleObjectsEx | WaitMessage => {
                Err(User32Error32::InvalidRequest)
            }
            SetWindowsHookExA | UnhookWindowsHookEx | CallNextHookEx => {
                self.hook_call(api, args, memory, context)
            }
            _ if api.id() >= 0x0002_00d0 => self.compatibility_call(api, args, memory, context),
            _ => self.input_query(api, args, memory, context),
        };
        let outcome = match result {
            Err(User32Error32::Win32(error)) => {
                self.last_error = Some(error);
                Ok(User32Outcome32::Return(api.failure_value()))
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
                Ok(User32Outcome32::Return(api.failure_value()))
            }
            Err(User32Error32::Capacity) => {
                self.last_error = Some(8);
                Ok(User32Outcome32::Return(api.failure_value()))
            }
            result => result,
        };
        if outcome.is_ok() {
            self.record_thread_layout(context.thread_id);
        }
        outcome
    }

    fn owned_input_window(&self, window: Option<WindowHandle32>, thread: u32) -> u32 {
        window
            .filter(|&window| {
                self.windows()
                    .get(window)
                    .is_ok_and(|record| record.description.thread_id == thread)
            })
            .map_or(0, WindowHandle32::get)
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
        let continuation = if let Continuation32::Call {
            procedure,
            arguments,
        } = continuation
        {
            if let Some(plan) = self.tooltip_subclass_call(procedure, arguments)? {
                Continuation32::TooltipRelay(plan)
            } else {
                continuation
            }
        } else {
            continuation
        };
        let token = self.check_continuation()?;
        self.next_token = token;
        self.pending.push(Pending32 {
            token,
            continuation,
            activation: None,
        });
        self.next_outcome()
    }
}
