use super::{encoding::payload, types::FontEnumerationCall32};
use crate::{
    memory::{Access, GuestAddress},
    process::{
        CallError, EngineInstance, HostError,
        calls::call::PendingOwner,
        windows::callbacks::{
            GuestCallbackScratch32, WindowsCallbackProvider32, WindowsProviderOutcome32,
        },
    },
    windows::{
        CallFrame32, CallingConvention32,
        gdi::{GdiApi32, GdiError32, LogicalFont32},
    },
};
impl EngineInstance {
    pub(in crate::process::windows::gdi) fn complete_gdi_font_enumeration(
        &mut self,
        owner: PendingOwner,
        token: u32,
        api: GdiApi32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        self.validate_windows_provider_owner(owner)?;
        if self.checked_pending_call(owner, token)?.frame != frame
            || frame.convention() != CallingConvention32::Stdcall
            || frame.stack_words() != api.stack_words()
        {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        Self::prepare_completed_call(frame.complete(0))?;
        self.check_windows_callback_capacity()?;
        self.validate_new_windows_reply(owner, token)?;
        let result = self.begin_gdi_font_enumeration(owner, token, api, frame);
        match result {
            Ok(WindowsProviderOutcome32::Callback(request)) => self.park_gdi_callback(request)?,
            Ok(WindowsProviderOutcome32::Return(value)) => {
                self.publish_prepared_call(Self::prepare_completed_call(frame.complete(value))?)
            }
            Err(HostError::Gdi(GdiError32::Memory(error))) => return Err(HostError::Memory(error)),
            Err(HostError::Gdi(_)) => {
                self.publish_prepared_call(Self::prepare_completed_call(frame.complete(0))?)
            }
            Err(error) => return Err(error),
        }
        Ok(true)
    }
    fn begin_gdi_font_enumeration(
        &mut self,
        owner: PendingOwner,
        token: u32,
        api: GdiApi32,
        frame: CallFrame32,
    ) -> Result<WindowsProviderOutcome32, HostError> {
        let args = frame.arguments();
        let wide = matches!(
            api,
            GdiApi32::EnumFontsW | GdiApi32::EnumFontFamiliesW | GdiApi32::EnumFontFamiliesExW
        );
        let cp = self.windows_ansi_code_page();
        let extended = matches!(
            api,
            GdiApi32::EnumFontFamiliesExA | GdiApi32::EnumFontFamiliesExW
        );
        let filter = if extended {
            if args[4] != 0 {
                return Err(HostError::Gdi(GdiError32::InvalidRequest));
            }
            super::super::memory::logical_font(self.memory()?, args[1], wide, cp)
                .map_err(HostError::Gdi)?
        } else {
            LogicalFont32 {
                face: if args[1] == 0 {
                    Vec::new()
                } else {
                    super::super::memory::string(self.memory()?, args[1], wide, -1, cp, 31)
                        .map_err(HostError::Gdi)?
                },
                charset: 1,
                ..Default::default()
            }
        };
        self.memory()?
            .check_span(GuestAddress(args[2]), 1, Access::Execute)
            .map_err(HostError::Memory)?;
        let runtime = self.gdi.as_ref().ok_or(HostError::InvalidRequest)?;
        runtime.model.dc_kind(args[0]).map_err(HostError::Gdi)?;
        if runtime.font_enumerations.len() >= self.windows_callback_work_capacity() {
            return Err(HostError::Call(CallError::Busy));
        }
        let items = runtime
            .fonts
            .enumerate_fonts(&filter)
            .map_err(HostError::Gdi)?;
        if items.is_empty() {
            return Ok(WindowsProviderOutcome32::Return(1));
        }
        let continuation = runtime
            .next_font_enumeration
            .checked_add(1)
            .ok_or(HostError::Gdi(GdiError32::Exhausted))?;
        let bounds = runtime.bounds;
        let thread = self.current_windows_thread_id()?;
        self.gdi
            .as_mut()
            .unwrap()
            .font_enumerations
            .try_reserve(1)
            .map_err(|_| HostError::Gdi(GdiError32::Allocation))?;
        let (bytes, metrics_offset) = payload(&items[0], wide, cp).map_err(HostError::Gdi)?;
        let scratch =
            GuestCallbackScratch32::allocate(self.memory.as_mut().unwrap(), bounds, &bytes)
                .map_err(HostError::Memory)?;
        let call = FontEnumerationCall32 {
            owner,
            token,
            frame,
            continuation,
            thread,
            procedure: GuestAddress(args[2]),
            parameter: args[3],
            items,
            index: 0,
            scratch,
            wide,
            cp,
            metrics_offset,
        };
        let request = call.request();
        let runtime = self.gdi.as_mut().unwrap();
        runtime.font_enumerations.push(call);
        runtime.next_font_enumeration = continuation;
        Ok(WindowsProviderOutcome32::Callback(request))
    }
    pub(in crate::process) fn check_gdi_font_callback(
        &self,
        owner: PendingOwner,
        token: u32,
        frame: CallFrame32,
        continuation: u64,
    ) -> Result<(), HostError> {
        let pending = self.checked_pending_call(owner, token)?;
        let thread = self.current_windows_thread_id()?;
        if pending.frame != frame {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        let call = self
            .gdi
            .as_ref()
            .ok_or(HostError::InvalidRequest)?
            .font_enumerations
            .iter()
            .find(|call| {
                call.continuation == continuation
                    && call.thread == thread
                    && call.owner == owner
                    && call.token == token
                    && call.frame == frame
            })
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        call.scratch
            .check(self.memory()?)
            .map_err(HostError::Memory)
    }
    pub(in crate::process) fn resume_gdi_callback(
        &mut self,
        continuation: u64,
        result: u32,
    ) -> Result<WindowsProviderOutcome32, HostError> {
        let thread = self.current_windows_thread_id()?;
        let runtime = self.gdi.as_ref().ok_or(HostError::InvalidRequest)?;
        let index = runtime
            .font_enumerations
            .iter()
            .position(|call| call.thread == thread && call.continuation == continuation)
            .ok_or(HostError::Call(CallError::InvalidToken))?;
        let call = &runtime.font_enumerations[index];
        self.check_gdi_font_callback(call.owner, call.token, call.frame, continuation)?;
        if self.windows_replies.last().and_then(|reply| {
            reply.received_result(
                WindowsCallbackProvider32::Gdi,
                call.owner,
                call.token,
                continuation,
                call.frame,
            )
        }) != Some(result)
        {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        if result == 0 || call.index + 1 == call.items.len() {
            call.scratch
                .release(self.memory.as_mut().unwrap())
                .map_err(HostError::Memory)?;
            self.gdi.as_mut().unwrap().font_enumerations.remove(index);
            return Ok(WindowsProviderOutcome32::Return(result));
        }
        let (bytes, metrics_offset) =
            payload(&call.items[call.index + 1], call.wide, call.cp).map_err(HostError::Gdi)?;
        self.memory
            .as_mut()
            .unwrap()
            .write(call.scratch.address, &bytes)
            .map_err(HostError::Memory)?;
        let call = &mut self.gdi.as_mut().unwrap().font_enumerations[index];
        call.index += 1;
        call.metrics_offset = metrics_offset;
        Ok(WindowsProviderOutcome32::Callback(call.request()))
    }
}
