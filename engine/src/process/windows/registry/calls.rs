use super::{RegistryError32 as E, RegistryReply32};
use crate::{
    process::{CallError, EngineInstance, HostError, calls::call::PendingOwner},
    windows::{CallFrame32, CallingConvention32, registry::Advapi32Api32 as Api},
};
impl EngineInstance {
    pub(in crate::process) fn try_complete_registry_call(
        &mut self,
        owner: PendingOwner,
        token: u32,
        id: u32,
        frame: CallFrame32,
    ) -> Result<bool, HostError> {
        let Some(api) = Api::from_id(id) else {
            return Ok(false);
        };
        self.checked_pending_call(owner, token)?;
        self.validate_windows_provider_owner(owner)?;
        if frame.convention() != CallingConvention32::Stdcall
            || frame.stack_words() != api.stack_words()
        {
            return Err(HostError::Call(CallError::InvalidRequest));
        }
        Self::prepare_completed_call(frame.complete(0))?;
        let thread = self.current_windows_thread_id()?;
        if api == Api::RegNotifyChangeKeyValue {
            match self.begin_registry_notify(owner, token, frame, thread) {
                Ok(false) => return Ok(true),
                Ok(true) => {
                    self.publish_prepared_call(Self::prepare_completed_call(frame.complete(0))?);
                    return Ok(true);
                }
                Err(error) => {
                    self.publish_prepared_call(Self::prepare_completed_call(
                        frame.complete(error.win32()),
                    )?);
                    return Ok(true);
                }
            }
        }
        if api == Api::RegFlushKey {
            let runtime = self
                .registry
                .as_mut()
                .ok_or(HostError::Registry(E::NotConfigured))?;
            match runtime.native.can_flush(frame.arguments()[0]) {
                Ok(false) => {
                    if runtime.reply.is_some() {
                        return Err(HostError::Call(CallError::Busy));
                    }
                    runtime.reply = Some(RegistryReply32 {
                        owner,
                        token,
                        frame,
                        thread,
                        wait: super::context::RegistryWaitKind32::Flush,
                    });
                    return Ok(true);
                }
                Ok(true) => {}
                Err(error) => {
                    self.publish_prepared_call(Self::prepare_completed_call(
                        frame.complete(error.win32()),
                    )?);
                    return Ok(true);
                }
            }
        }
        let time = self.windows_filetime()?;
        let value = self
            .registry_call(api, frame.arguments(), time)
            .unwrap_or_else(E::win32);
        self.publish_prepared_call(Self::prepare_completed_call(frame.complete(value))?);
        Ok(true)
    }
    pub(super) fn registry_call(&mut self, api: Api, args: &[u32], time: u64) -> Result<u32, E> {
        match api {
            Api::RegOpenKeyExA
            | Api::RegOpenKeyExW
            | Api::RegOpenKeyA
            | Api::RegOpenKeyW
            | Api::RegCreateKeyExA
            | Api::RegCreateKeyExW
            | Api::RegCreateKeyA
            | Api::RegCreateKeyW
            | Api::RegConnectRegistryA
            | Api::RegConnectRegistryW
            | Api::RegOpenCurrentUser => self.registry_open(api, args, time),
            Api::RegSetValueExA
            | Api::RegSetValueExW
            | Api::RegSetValueA
            | Api::RegSetValueW
            | Api::RegDeleteValueA
            | Api::RegDeleteValueW
            | Api::RegDeleteKeyA
            | Api::RegDeleteKeyW
            | Api::RegDeleteKeyExA
            | Api::RegDeleteKeyExW
            | Api::RegDeleteTreeA
            | Api::RegDeleteTreeW
            | Api::RegSetKeyValueA
            | Api::RegSetKeyValueW
            | Api::RegDeleteKeyValueA
            | Api::RegDeleteKeyValueW
            | Api::RegCopyTreeA
            | Api::RegCopyTreeW => self.registry_mutation(api, args, time),
            Api::RegQueryValueExA
            | Api::RegQueryValueExW
            | Api::RegQueryValueA
            | Api::RegQueryValueW
            | Api::RegGetValueA
            | Api::RegGetValueW => self.registry_value(api, args),
            Api::RegEnumKeyExA
            | Api::RegEnumKeyExW
            | Api::RegEnumKeyA
            | Api::RegEnumKeyW
            | Api::RegEnumValueA
            | Api::RegEnumValueW
            | Api::RegQueryInfoKeyA
            | Api::RegQueryInfoKeyW => self.registry_enumeration(api, args),
            Api::RegQueryMultipleValuesA | Api::RegQueryMultipleValuesW => {
                self.registry_multiple(api, args)
            }
            Api::RegCloseKey => {
                let runtime = self.registry.as_mut().ok_or(E::NotConfigured)?;
                let plan = runtime.prepare_close_notifications(
                    args[0],
                    self.synchronization.as_ref().map(|sync| sync.context()),
                )?;
                runtime.native.close(args[0])?;
                runtime.commit_notifications(
                    plan,
                    self.synchronization.as_mut().map(|sync| sync.context_mut()),
                );
                Ok(0)
            }
            Api::RegOverridePredefKey => {
                self.registry
                    .as_mut()
                    .ok_or(E::NotConfigured)?
                    .native
                    .override_predefined(args[0], args[1])?;
                Ok(0)
            }
            Api::RegDisablePredefinedCache | Api::RegDisablePredefinedCacheEx => {
                self.registry
                    .as_mut()
                    .ok_or(E::NotConfigured)?
                    .native
                    .disable_predefined_cache(api == Api::RegDisablePredefinedCacheEx);
                Ok(0)
            }
            Api::RegOpenUserClassesRoot => {
                if args[1] != 0 {
                    return Err(E::InvalidParameter);
                }
                Err(E::InvalidHandle)
            }
            Api::RegFlushKey => {
                if self
                    .registry
                    .as_ref()
                    .ok_or(E::NotConfigured)?
                    .native
                    .can_flush(args[0])?
                {
                    Ok(0)
                } else {
                    Err(E::Busy)
                }
            }
            Api::RegNotifyChangeKeyValue => Err(E::InvalidParameter),
        }
    }
}
