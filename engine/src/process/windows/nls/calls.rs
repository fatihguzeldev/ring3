use crate::{
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    windows::{
        CallFrame32,
        nls::{NlsApi32, NlsError32},
    },
};
impl EngineInstance {
    pub(in crate::process) fn try_complete_nls_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let Some(api) = NlsApi32::from_id(id) else {
            return Ok(false);
        };
        self.validate_windows_provider_owner(owner)?;
        self.checked_pending_call(owner, token)?;
        if frame.convention() != api.convention() || frame.stack_words() != api.stack_words() {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        Self::prepare_completed_call(frame.complete(0))?;
        self.check_windows_thread_write()?;
        self.memory()?
            .ensure_transaction_versions(2)
            .map_err(HostError::Memory)?;
        let (value, error) = match self.execute_nls_call(api, frame.arguments()) {
            Ok(value) => (value, None),
            Err(NlsError32::Memory(error)) => return Err(HostError::Memory(error)),
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
    pub(super) fn execute_nls_call(
        &mut self,
        api: NlsApi32,
        args: &[u32],
    ) -> Result<u32, NlsError32> {
        use NlsApi32::*;
        match api {
            GetLocaleInfoEx
            | GetUserDefaultLocaleName
            | IsValidLocaleName
            | LCIDToLocaleName
            | LocaleNameToLCID => self.nls.names(
                self.memory.as_mut().unwrap(),
                self.threads.current_id(),
                api,
                args,
            ),
            MultiByteToWideChar | WideCharToMultiByte => self.nls.convert(
                self.memory.as_mut().unwrap(),
                api,
                self.threads.current_id(),
                args,
            ),
            GetACP | GetOEMCP | IsValidCodePage | GetCPInfo | GetCPInfoExA | GetCPInfoExW
            | IsDBCSLeadByte | IsDBCSLeadByteEx => self.nls.codepages(
                self.memory.as_mut().unwrap(),
                api,
                self.threads.current_id(),
                args,
            ),
            LCMapStringA | LCMapStringW | CompareStringA | CompareStringW
            | CompareStringOrdinal | GetStringTypeA | GetStringTypeW | GetStringTypeExA
            | GetStringTypeExW | LstrlenA | LstrlenW | LstrcmpA | LstrcmpW | LstrcmpiA
            | LstrcmpiW | LstrcpyA | LstrcpyW | LstrcpynA | LstrcpynW | LstrcatA | LstrcatW => self
                .nls
                .linguistic(self.memory.as_mut().unwrap(), api, args),
            GetSystemTime
            | GetLocalTime
            | SystemTimeToFileTime
            | FileTimeToSystemTime
            | FileTimeToLocalFileTime
            | LocalFileTimeToFileTime
            | CompareFileTime
            | GetTimeZoneInformation
            | SystemTimeToTzSpecificLocalTime
            | TzSpecificLocalTimeToSystemTime
            | DosDateTimeToFileTime
            | FileTimeToDosDateTime
            | GetDateFormatA
            | GetDateFormatW
            | GetTimeFormatA
            | GetTimeFormatW => {
                let now = if matches!(api, GetSystemTime | GetLocalTime)
                    || matches!(
                        api,
                        GetDateFormatA | GetDateFormatW | GetTimeFormatA | GetTimeFormatW
                    ) && args[2] == 0
                {
                    self.windows_filetime()
                        .map_err(|_| NlsError32::InvalidParameter)?
                } else {
                    0
                };
                self.nls
                    .calendar(self.memory.as_mut().unwrap(), api, args, now)
            }
            _ => self.nls.locale_call(
                self.memory.as_mut().unwrap(),
                self.threads.current_id(),
                api,
                args,
            ),
        }
    }
}
