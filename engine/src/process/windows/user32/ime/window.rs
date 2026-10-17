use super::super::{User32CallContext32, User32Context32, User32Outcome32};
use crate::{
    memory::AddressSpace,
    process::{EngineInstance, HostError},
    windows::{
        imm32::ImmApi32 as Api,
        user32::{User32Api32, User32Error32 as Error},
        window::WindowHandle32,
    },
};

impl EngineInstance {
    pub(in crate::process) fn default_ime_window(
        &mut self,
        raw: u32,
        current_thread: u32,
    ) -> Result<u32, HostError> {
        let Some(context) = self.user32.as_ref() else {
            return Ok(0);
        };
        let thread = if raw == 0 {
            current_thread
        } else {
            let Some(record) = WindowHandle32::try_from(raw)
                .ok()
                .and_then(|window| context.windows().get(window).ok())
            else {
                return Ok(0);
            };
            record.description.thread_id
        };
        if let Some(window) = context.state.default_ime_window(thread) {
            return Ok(window.get());
        }
        let procedure = self
            .loaded_windows_modules()
            .flat_map(|module| module.gates())
            .find(|gate| gate.id == User32Api32::DefWindowProcW.id())
            .map(|gate| gate.entry);
        let Some(procedure) = procedure else {
            return Ok(0);
        };
        let Some((instance, _)) = self.prepare_module_handle_reference(None, Some(procedure), 6)?
        else {
            return Ok(0);
        };
        let retain = context.state.class(instance, &[73, 77, 69], None).is_err();
        let (_, reference) = self
            .prepare_module_handle_reference(None, Some(procedure), if retain { 4 } else { 6 })?
            .ok_or(HostError::InvalidRequest)?;
        let queue = self.prepare_user32_queue_change_for(thread)?;
        let window = self
            .user32
            .as_mut()
            .unwrap()
            .state
            .create_default_ime_window(thread, instance, procedure)
            .map_err(HostError::User32)?;
        if retain {
            self.commit_module_handle_reference(reference);
        }
        self.commit_user32_queue_change(queue);
        Ok(window.get())
    }
}

impl User32Context32 {
    pub(in crate::process::windows::user32) fn ime_control_message(
        &mut self,
        args: &[u32],
        wide: bool,
        memory: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<Option<User32Outcome32>, Error> {
        if args[1] != 0x283 {
            return Ok(None);
        }
        let handle = self.owned(args[0], call.thread_id)?;
        if !self.state.is_ime_window(handle)? {
            return Ok(None);
        }
        let target = self.windows().focus().and_then(|window| {
            self.windows()
                .get(window)
                .ok()
                .filter(|record| record.description.thread_id == call.thread_id)
                .map(|_| window.get())
        });
        let input = self
            .ime
            .window_context(target.unwrap_or(handle.get()), call.thread_id)
            .map_err(|_| Error::Win32(87))?;
        if input == 0 {
            return Ok(Some(User32Outcome32::Return(1)));
        }
        let api = match args[2] {
            7 => Api::ImmGetCandidateWindow,
            8 => Api::ImmSetCandidateWindow,
            9 => {
                if wide {
                    Api::ImmGetCompositionFontW
                } else {
                    Api::ImmGetCompositionFontA
                }
            }
            10 => {
                if wide {
                    Api::ImmSetCompositionFontW
                } else {
                    Api::ImmSetCompositionFontA
                }
            }
            11 => Api::ImmGetCompositionWindow,
            12 => Api::ImmSetCompositionWindow,
            _ => return Ok(Some(User32Outcome32::Return(1))),
        };
        let words = if args[2] == 7 {
            let index = super::super::memory::words::<1>(memory, args[3])?[0];
            [input, index, args[3]]
        } else {
            [input, args[3], 0]
        };
        let success = self.complete_imm32_call(api, &words, call.thread_id, call.ansi, memory)?;
        Ok(Some(User32Outcome32::Return(u32::from(success == 0))))
    }
}
