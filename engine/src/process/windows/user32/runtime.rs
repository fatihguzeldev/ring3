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
            || !self.user32_reply.is_empty()
        {
            return Err(HostError::User32(User32Error32::Busy));
        }
        let context = User32Context32::new(limits, scratch_bounds).map_err(HostError::User32)?;
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
        let context = self.user32.as_mut().ok_or(HostError::InvalidRequest)?;
        if message.thread_id != 1 || message.message > 0xffff {
            return Err(HostError::InvalidRequest);
        }
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
        self.reserve_windows_reply()?;
        self.check_windows_thread_write()?;
        Self::prepare_completed_call(frame.complete(0))?;
        let call_context = self.user32_call_context()?;
        let flags = self
            .user32
            .as_ref()
            .and_then(|context| context.preview_input_flags(api, &frame, call_context));
        let input = flags
            .map(|(focused, captured)| self.prepare_windows_input_flags(focused, captured))
            .transpose()?
            .flatten();
        let context = self
            .user32
            .as_mut()
            .ok_or(HostError::User32(User32Error32::InvalidRequest))?;
        let outcome = context
            .prepare(
                api,
                &frame,
                self.memory.as_mut().ok_or(HostError::Closed)?,
                call_context,
            )
            .map_err(HostError::User32)?;
        let error = context.take_last_error();
        let actual = (
            context.windows().focus().is_some(),
            context.windows().capture().is_some(),
        );
        self.commit_windows_input_flags(if flags == Some(actual) { input } else { None });
        self.publish_user32_result(owner, token, frame, outcome, error)?;
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
            thread_id: 1,
            instance: self.image.map_or(0, |image| image.image_base),
            time_ms,
            ..context.runtime_input
        })
    }
}
