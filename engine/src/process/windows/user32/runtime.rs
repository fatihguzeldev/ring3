use super::{User32CallContext32, User32Context32};
use crate::{
    memory::PageRange,
    process::{EngineInstance, HostError, calls::call::PendingOwner},
    windows::{
        CallFrame32,
        user32::{Message32, User32Api32, User32Error32, User32Limits32},
        window::WindowContext32,
    },
};

impl EngineInstance {
    pub fn configure_user32(
        &mut self,
        limits: User32Limits32,
        scratch_bounds: PageRange,
    ) -> Result<(), HostError> {
        self.memory()?;
        if self.user32.is_some()
            || self.pending_call.is_some()
            || self.callback.is_some()
            || !self.windows_replies.is_empty()
        {
            return Err(HostError::User32(User32Error32::Busy));
        }
        self.check_guest_reservation_admission(&[scratch_bounds])?;
        let mut context =
            User32Context32::new(limits, scratch_bounds).map_err(HostError::User32)?;
        if let Some(media) = self.media.as_ref() {
            context
                .input
                .attach(media.input.source().tail(), media.input.source().snapshot());
        }
        let input = self.prepare_windows_input_flags(false, false)?;
        self.user32 = Some(context);
        self.commit_windows_input_flags(input);
        Ok(())
    }

    pub fn register_user32_module(&mut self, preferred_base: u32) -> Result<(), HostError> {
        self.register_system_module(crate::windows::user32::user32_module(preferred_base))
    }

    pub fn user32_windows(&self) -> Option<&WindowContext32> {
        self.user32.as_ref().map(User32Context32::windows)
    }

    pub fn set_user32_keyboard_state(
        &mut self,
        shift: bool,
        control: bool,
        caps_lock: bool,
    ) -> Result<(), HostError> {
        self.memory()?;
        let context = self.user32.as_mut().ok_or(HostError::InvalidRequest)?;
        context.runtime_input.shift = shift;
        context.runtime_input.control = control;
        context.runtime_input.caps_lock = caps_lock;
        Ok(())
    }

    pub fn post_user32_message(&mut self, mut message: Message32) -> Result<(), HostError> {
        self.memory()?;
        if message.thread_id == 0 || message.message > 0xffff {
            return Err(HostError::InvalidRequest);
        }
        let thread = self
            .threads
            .info(message.thread_id)
            .map_err(HostError::Thread)?;
        if thread.state == crate::windows::thread::ThreadState32::Terminated {
            return Err(HostError::InvalidRequest);
        }
        let queue = self.prepare_user32_queue_change_for(message.thread_id)?;
        let context = self.user32.as_mut().ok_or(HostError::InvalidRequest)?;
        if message.window != 0 {
            let handle = crate::windows::window::WindowHandle32::try_from(message.window)
                .map_err(|error| HostError::User32(error.into()))?;
            if context
                .windows()
                .get(handle)
                .map_err(|error| HostError::User32(error.into()))?
                .description
                .thread_id
                != message.thread_id
            {
                return Err(HostError::InvalidRequest);
            }
        }
        let point = message.point;
        if message.message == 0x12 {
            message.window = 0;
            context
                .state
                .post_quit(message)
                .map_err(HostError::User32)?;
        } else {
            context.state.post(message).map_err(HostError::User32)?;
        }
        context.runtime_input.cursor = point;
        self.commit_user32_queue_change(queue);
        Ok(())
    }

    pub(in crate::process) fn try_complete_user32_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let Some(api) = User32Api32::from_id(id) else {
            return Ok(false);
        };
        self.validate_windows_provider_owner(owner)?;
        self.validate_new_windows_reply(owner, token)?;
        if self.checked_pending_call(owner, token)?.frame != frame {
            return Err(HostError::InvalidRequest);
        }
        self.refresh_user32_input()?;
        self.reserve_windows_reply()?;
        self.check_windows_thread_write()?;
        Self::prepare_completed_call(frame.complete(0))?;
        let call_context = self.user32_call_context()?;
        let message_phase = matches!(
            api,
            User32Api32::SendMessageA
                | User32Api32::SendMessageW
                | User32Api32::GetMessageA
                | User32Api32::GetMessageW
                | User32Api32::PeekMessageA
                | User32Api32::PeekMessageW
                | User32Api32::MessageBoxA
                | User32Api32::MessageBoxW
                | User32Api32::MessageBoxIndirectA
                | User32Api32::MessageBoxIndirectW
        );
        if message_phase {
            self.check_thread_message_dispatch(call_context.thread_id)?;
        }
        let (remote, remote_error) =
            match self.prepare_user32_remote_call(api, &frame, call_context) {
                Ok(value) => (value, None),
                Err(HostError::Thread(error)) => (
                    None,
                    Some(
                        if matches!(
                            error,
                            crate::windows::thread::ThreadError32::InvalidHandle
                                | crate::windows::thread::ThreadError32::Stale
                        ) {
                            1400
                        } else {
                            error.win32()
                        },
                    ),
                ),
                Err(error) => return Err(error),
            };
        let defer = matches!(
            api,
            User32Api32::GetMessageA
                | User32Api32::GetMessageW
                | User32Api32::PeekMessageA
                | User32Api32::PeekMessageW
        ) && self.has_pending_thread_invocation(call_context.thread_id);

        let flags = self
            .user32
            .as_ref()
            .and_then(|context| context.preview_input_flags(api, &frame, call_context));
        let input = flags
            .map(|(focused, captured)| self.prepare_windows_input_flags(focused, captured))
            .transpose()?
            .flatten();
        let window_ids = self.user32_window_identities();
        let translation = matches!(
            api,
            User32Api32::ActivateKeyboardLayout
                | User32Api32::LoadKeyboardLayoutA
                | User32Api32::LoadKeyboardLayoutW
                | User32Api32::SwapMouseButton
        );
        let previous_translation = self.user32_translation_settings();
        let window_event = if flags.is_some()
            || translation
            || matches!(api, User32Api32::SetCursorPos | User32Api32::ClipCursor)
        {
            self.prepare_user32_window_event()?
        } else {
            None
        };
        let queue_thread = if matches!(api, User32Api32::PostMessageA | User32Api32::PostMessageW) {
            frame
                .arguments()
                .first()
                .and_then(|&raw| crate::windows::window::WindowHandle32::try_from(raw).ok())
                .and_then(|window| self.user32.as_ref()?.windows().get(window).ok())
                .map_or(call_context.thread_id, |window| {
                    window.description.thread_id
                })
        } else {
            call_context.thread_id
        };
        let queue = if matches!(
            api,
            User32Api32::PeekMessageA
                | User32Api32::PeekMessageW
                | User32Api32::GetMessageA
                | User32Api32::GetMessageW
                | User32Api32::TranslateMessage
                | User32Api32::PostMessageA
                | User32Api32::PostMessageW
                | User32Api32::PostQuitMessage
                | User32Api32::GetQueueStatus
                | User32Api32::GetInputState
        ) {
            self.prepare_user32_queue_change_for(queue_thread)?
        } else {
            None
        };
        let dialog = if matches!(
            api,
            User32Api32::MessageBoxA
                | User32Api32::MessageBoxW
                | User32Api32::MessageBoxIndirectA
                | User32Api32::MessageBoxIndirectW
        ) {
            Some(self.prepare_user32_dialog(api, frame.arguments(), call_context))
        } else {
            None
        };
        let activation = if matches!(
            api,
            User32Api32::PostMessageA | User32Api32::PostMessageW | User32Api32::SetTimer
        ) {
            Some(self.prepare_managed_activation_capture()?)
        } else {
            None
        };
        let context = self
            .user32
            .as_mut()
            .ok_or(HostError::User32(User32Error32::InvalidRequest))?;
        context.call_activation = activation
            .as_ref()
            .map_or_else(Default::default, |capture| capture.token());
        let result = if let Some(dialog) = dialog {
            match dialog {
                Ok(dialog) => context.begin_dialog(dialog),
                Err(HostError::User32(error)) => Err(error),
                Err(error) => return Err(error),
            }
        } else if let Some(error) = remote_error {
            context.last_error = Some(error);
            Ok(super::User32Outcome32::Return(0))
        } else if defer {
            context.defer_retrieval(api, &frame, call_context)
        } else {
            context.prepare(
                api,
                &frame,
                self.memory.as_mut().ok_or(HostError::Closed)?,
                call_context,
            )
        };
        context.call_activation = Default::default();
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(User32Error32::Win32(error)) => {
                context.last_error = Some(error);
                super::User32Outcome32::Return(
                    if matches!(api, User32Api32::GetMessageA | User32Api32::GetMessageW) {
                        u32::MAX
                    } else {
                        0
                    },
                )
            }
            Err(User32Error32::Capacity) => {
                context.last_error = Some(8);
                super::User32Outcome32::Return(0)
            }
            Err(error) => return Err(HostError::User32(error)),
        };
        let error = context.take_last_error();
        let actual = (
            context.windows().focus().is_some(),
            context.windows().capture().is_some(),
        );
        if error.is_none()
            && matches!(outcome, super::User32Outcome32::Return(value) if value != 0)
            && let Some(activation) = activation
        {
            self.commit_managed_activation_capture(activation);
        }
        if let Some((prepared, continuation)) = remote {
            assert_eq!(outcome, super::User32Outcome32::Wait { continuation });
            self.commit_thread_invocation(prepared);
        }
        if message_phase {
            self.commit_thread_message_dispatch(call_context.thread_id, error.is_none());
        }
        self.commit_windows_input_flags(if flags == Some(actual) { input } else { None });
        if matches!(api, User32Api32::SetCursorPos | User32Api32::ClipCursor)
            && outcome == super::User32Outcome32::Return(1)
        {
            self.commit_user32_cursor_event(window_event, api == User32Api32::ClipCursor);
        } else if translation {
            self.commit_user32_translation_event(window_event, previous_translation);
        } else {
            self.commit_user32_window_event(window_event, window_ids);
        }
        self.commit_user32_queue_change(queue);
        self.publish_user32_result(owner, token, frame, outcome, error)?;
        let _ = self.sync_windows_input();
        Ok(true)
    }

    pub(super) fn user32_call_context(&self) -> Result<User32CallContext32, HostError> {
        let context = self
            .user32
            .as_ref()
            .ok_or(HostError::User32(User32Error32::InvalidRequest))?;
        let time_ms = self
            .cpu_clock
            .monotonic_bounds()
            .map_or(0, |(origin, now)| ((now - origin) / 1_000_000) as u32);
        Ok(User32CallContext32 {
            thread_id: self.current_windows_thread_id()?,
            instance: self.image.map_or(0, |image| image.image_base),
            time_ms,
            now_ns: self.cpu_clock.monotonic_bounds().map_or(0, |(_, now)| now),
            ansi: self.windows_ansi_code_page(),
            ..context.runtime_input
        })
    }
}
