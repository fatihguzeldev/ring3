use super::super::{User32Context32, User32Outcome32};
use crate::{
    memory::AddressSpace,
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    windows::{
        CallFrame32, CallingConvention32, rawinput::RawInputApi32 as Api,
        user32::User32Error32 as E,
    },
};
impl EngineInstance {
    pub(in crate::process) fn try_complete_raw_input_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let Some(api) = Api::from_id(id) else {
            return Ok(false);
        };
        self.validate_windows_provider_owner(owner)?;
        self.validate_new_windows_reply(owner, token)?;
        if self.checked_pending_call(owner, token)?.frame != frame
            || frame.convention() != CallingConvention32::Stdcall
            || frame.stack_words() != api.stack_words()
        {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        self.refresh_user32_input()?;
        self.reserve_windows_reply()?;
        self.check_windows_thread_write()?;
        self.memory()?
            .ensure_transaction_versions(2)
            .map_err(HostError::Memory)?;
        Self::prepare_completed_call(frame.complete(0))?;
        let thread = self.current_windows_thread_id()?;
        let queue = if api == Api::GetRawInputBuffer {
            self.prepare_user32_queue_change_for(thread)?
        } else {
            None
        };
        let context = self
            .user32
            .as_mut()
            .ok_or(HostError::User32(E::InvalidRequest))?;
        let result = context.raw_input_call(
            api,
            frame.arguments(),
            self.memory.as_mut().ok_or(HostError::Closed)?,
            thread,
        );
        let (value, error) = match result {
            Ok(value) => (value, None),
            Err(error) => (
                api.failure_value(),
                Some(match error {
                    E::Win32(code) => code,
                    E::Memory(_) => 998,
                    E::Allocation | E::Capacity => 8,
                    _ => 87,
                }),
            ),
        };
        self.commit_user32_queue_change(queue);
        self.publish_user32_result(owner, token, frame, User32Outcome32::Return(value), error)?;
        Ok(true)
    }
}
impl User32Context32 {
    pub(in crate::process) fn terminate_raw_input(&mut self) {
        self.raw_input.terminate();
    }
    pub(in crate::process::windows::user32) fn raw_input_call(
        &mut self,
        api: Api,
        args: &[u32],
        memory: &mut AddressSpace,
        thread: u32,
    ) -> Result<u32, E> {
        if args.len() != api.stack_words() as usize {
            return Err(E::InvalidRequest);
        }
        match api {
            Api::RegisterRawInputDevices => self.register_raw_input(args, memory),
            Api::GetRegisteredRawInputDevices => self.registered_raw_input(args, memory),
            Api::GetRawInputData => self.read_raw_input(args, memory, thread),
            Api::GetRawInputBuffer => self.read_raw_input_buffer(args, memory, thread),
            Api::GetRawInputDeviceList => self.raw_input_device_list(args, memory),
            Api::GetRawInputDeviceInfoA | Api::GetRawInputDeviceInfoW => {
                self.raw_input_device_info(args, memory, api == Api::GetRawInputDeviceInfoW)
            }
            Api::DefRawInputProc => {
                if args[2] == 16 {
                    Ok(0)
                } else {
                    Err(E::Win32(87))
                }
            }
        }
    }
}
