use super::*;
use crate::{
    memory::{PAGE_SIZE, Permissions},
    process::{EngineInstance, HostError},
};

impl EngineInstance {
    pub(in crate::process) fn validate_file_mapping_protection(
        &self,
        range: PageRange,
        permissions: Permissions,
    ) -> Result<bool, FileError32> {
        self.files.as_ref().map_or(Ok(false), |files| {
            files.mappings.validate_view_protection(range, permissions)
        })
    }
    pub(super) fn try_complete_file_mapping_allocation(
        &mut self,
        owner: PendingOwner,
        token: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let Some(files) = self.files.as_ref() else {
            return Ok(false);
        };
        let args = frame.arguments();
        let Some((&address, &size)) = args.first().zip(args.get(1)) else {
            return Ok(false);
        };
        if address == 0 || size == 0 {
            return Ok(false);
        }
        let start = address & !(PAGE_SIZE - 1);
        let end = u64::from(address) + u64::from(size);
        if end > 1u64 << 32 {
            return Ok(false);
        }
        let range = PageRange::new(
            GuestAddress(start),
            ((end - u64::from(start)).div_ceil(u64::from(PAGE_SIZE))) as u32,
        )
        .map_err(HostError::Memory)?;
        if !files.mappings.contains_range(range) {
            return Ok(false);
        }
        self.validate_windows_provider_owner(owner)?;
        self.checked_pending_call(owner, token)?;
        if frame.convention() != crate::windows::CallingConvention32::Stdcall
            || frame.stack_words() != 4
        {
            return Err(HostError::Call(crate::process::CallError::InvalidRequest));
        }
        Self::prepare_completed_call(frame.complete(0))?;
        self.check_windows_thread_write()?;
        self.memory()?
            .ensure_transaction_versions(2)
            .map_err(HostError::Memory)?;
        let permissions = match args[3] {
            1 => Some(Permissions::NONE),
            2 => Some(Permissions::READ),
            4 => Some(Permissions::READ_WRITE),
            0x10 => Some(Permissions::EXECUTE),
            0x20 => Some(Permissions::READ_EXECUTE),
            0x40 => Some(Permissions::ALL),
            _ => None,
        };
        let result = if args[2] != 0x1000 {
            Err(FileError32::InvalidParameter)
        } else if let Some(permissions) = permissions {
            self.files.as_mut().unwrap().try_commit_reserved_view(
                self.memory.as_mut().unwrap(),
                range,
                permissions,
            )
        } else {
            Err(FileError32::InvalidParameter)
        };
        let (value, error) = match result {
            Ok(true) => (start, None),
            Ok(false) => (0, Some(487)),
            Err(error) => (0, Some(error.win32())),
        };
        let prepared = Self::prepare_completed_call(frame.complete(value))?;
        if let Some(error) = error {
            let thread = self.current_windows_thread()?.with_last_error(error);
            self.publish_windows_thread(thread)?;
        }
        self.publish_prepared_call(prepared);
        Ok(true)
    }
}
