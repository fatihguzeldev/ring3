use super::super::{memory::read_words, objects::ObjectKind32, runtime::ReplyKind32};
use crate::{
    graphics::d3d8::api::{InterfaceKind, MethodGate, PresentationParameters32},
    memory::GuestAddress,
    process::{
        EngineInstance, HostError, WindowsCallback32, WindowsCallbackProvider32,
        WindowsProviderOutcome32,
        windows::user32::{User32Callback32, User32Outcome32},
    },
    windows::window::WindowHandle32,
};

impl EngineInstance {
    pub(in crate::process::windows::d3d8) fn prepare_d3d8_window(
        &mut self,
        gate: Option<MethodGate>,
        arguments: &[u32],
    ) -> Result<Option<User32Callback32>, HostError> {
        let Some(gate) = gate else { return Ok(None) };
        let runtime = self.d3d8.as_ref().ok_or(HostError::Infrastructure)?;
        let (focus, parameters) = match (gate.interface, gate.slot) {
            (InterfaceKind::Direct3D, 15) => (arguments[3], arguments[5]),
            (InterfaceKind::Device, 14) => {
                let Ok(object) = runtime.object(GuestAddress(arguments[0])) else {
                    return Ok(None);
                };
                let ObjectKind32::Device(device) = &object.kind else {
                    return Ok(None);
                };
                (device.focus_window, arguments[1])
            }
            _ => return Ok(None),
        };
        // invalid parameters remain the backend's HRESULT responsibility.
        let Ok(words) = read_words(self.memory()?, parameters) else {
            return Ok(None);
        };
        let input = PresentationParameters32::decode(words);
        if input.windowed {
            return Ok(None);
        }
        let raw = if input.window == 0 {
            focus
        } else {
            input.window
        };
        let Some(windows) = self.user32.as_ref().map(|context| context.windows()) else {
            return Ok(None);
        };
        let Ok(window) = WindowHandle32::try_from(raw).and_then(|window| windows.get(window))
        else {
            return Ok(None);
        };
        let Ok(presentation) = input.normalize(
            focus,
            window.description.extent,
            &runtime.config.adapter,
            false,
        ) else {
            return Ok(None);
        };
        let context = self.user32_call_context()?;
        // position continuations execute on their window's thread. preserve the
        // existing device path until cross-thread geometry can be forwarded.
        if window.description.thread_id != context.thread_id {
            return Ok(None);
        }
        self.user32
            .as_mut()
            .unwrap()
            .begin_presentation_position(
                presentation.window,
                presentation.width,
                presentation.height,
                self.memory.as_mut().ok_or(HostError::Closed)?,
                context,
            )
            .map(Some)
            .map_err(HostError::User32)
    }

    pub(in crate::process::windows::d3d8) fn cancel_d3d8_window(
        &mut self,
    ) -> Result<(), HostError> {
        if let Some(window) = self.d3d8_reply.and_then(|reply| reply.window) {
            self.user32
                .as_mut()
                .ok_or(HostError::Infrastructure)?
                .cancel_presentation_position(
                    window.continuation,
                    self.memory.as_mut().ok_or(HostError::Closed)?,
                )
                .map_err(HostError::User32)?;
            self.d3d8_reply = None;
        }
        Ok(())
    }

    pub(in crate::process::windows::d3d8) fn d3d8_window_outcome(
        &mut self,
        result: u32,
    ) -> Result<WindowsProviderOutcome32, HostError> {
        let Some(window) = self.d3d8_reply.and_then(|reply| reply.window) else {
            return Ok(WindowsProviderOutcome32::Return(result));
        };
        if result != 0 {
            self.cancel_d3d8_window()?;
            return Ok(WindowsProviderOutcome32::Return(result));
        }
        self.d3d8_reply.as_mut().unwrap().kind = ReplyKind32::Window;
        Ok(WindowsProviderOutcome32::Callback(window_callback(window)))
    }

    pub(in crate::process::windows::d3d8) fn resume_d3d8_window(
        &mut self,
        continuation: u64,
        result: u32,
    ) -> Result<WindowsProviderOutcome32, HostError> {
        let expected = self
            .d3d8_reply
            .and_then(|reply| reply.window)
            .ok_or(HostError::Infrastructure)?;
        if expected.continuation != continuation {
            return Err(HostError::InvalidRequest);
        }
        let outcome = self
            .user32
            .as_mut()
            .ok_or(HostError::Infrastructure)?
            .complete_with_gdi(
                continuation,
                result,
                self.memory.as_mut().ok_or(HostError::Closed)?,
                self.gdi.as_mut(),
            )?;
        match outcome {
            User32Outcome32::Callback(window) => {
                self.d3d8_reply.as_mut().unwrap().window = Some(window);
                Ok(WindowsProviderOutcome32::Callback(window_callback(window)))
            }
            User32Outcome32::Return(result) => {
                self.d3d8_reply = None;
                Ok(WindowsProviderOutcome32::Return(result))
            }
            User32Outcome32::Wait { .. } => Err(HostError::Infrastructure),
        }
    }
}

fn window_callback(request: User32Callback32) -> WindowsCallback32 {
    let mut arguments = [0; 16];
    arguments[..4].copy_from_slice(&request.arguments);
    WindowsCallback32 {
        procedure: request.procedure,
        arguments,
        count: request.count,
        continuation: request.continuation,
        provider: WindowsCallbackProvider32::D3D8,
    }
}
