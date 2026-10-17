use super::Enumeration32;
use crate::{
    memory::{Access, GuestAddress},
    process::{
        EngineInstance, HostError,
        windows::{
            callbacks::{GuestCallbackScratch32, WindowsProviderOutcome32},
            nls::NlsError32,
        },
    },
    windows::nls::locales::{installed_locales, locale_is_neutral},
};
impl EngineInstance {
    pub(in crate::process::windows::nls) fn begin_nls_enumeration(
        &mut self,
        args: &[u32],
    ) -> Result<WindowsProviderOutcome32, HostError> {
        self.begin_nls_enumeration_format(args, false)
    }
    pub(in crate::process::windows::nls) fn begin_nls_legacy_enumeration(
        &mut self,
        args: &[u32],
    ) -> Result<WindowsProviderOutcome32, HostError> {
        if !matches!(args[1] & !4, 0 | 1 | 2) || args[1] & !7 != 0 {
            return Err(HostError::Nls(NlsError32::InvalidFlags));
        }
        self.begin_nls_enumeration_format(
            &[
                args[0],
                if args[1] == 4 {
                    4
                } else if args[1] & 4 != 0 {
                    5
                } else {
                    1
                },
                0,
                0,
            ],
            true,
        )
    }
    fn begin_nls_enumeration_format(
        &mut self,
        args: &[u32],
        legacy: bool,
    ) -> Result<WindowsProviderOutcome32, HostError> {
        let failure = |error| HostError::Nls(error);
        if args[0] == 0 || args[3] != 0 {
            return Err(failure(NlsError32::InvalidParameter));
        }
        if args[1] & !0x37 != 0 {
            return Err(failure(NlsError32::InvalidFlags));
        }
        self.memory()?
            .check_span(GuestAddress(args[0]), 1, Access::Execute)
            .map_err(HostError::Memory)?;
        if self.nls.enumerations.len() >= 64 {
            return Err(failure(NlsError32::Capacity));
        }
        let mut items = Vec::new();
        for locale in installed_locales() {
            if locale.lcid == 0x7f {
                continue;
            }
            let kind = if locale.alternate_sort() { 4 } else { 1 };
            let region = if locale_is_neutral(locale) {
                0x10
            } else {
                0x20
            };
            let category = args[1] & 7;
            let data = args[1] & 0x30;
            if (category == 0 && kind != 4 || category & kind != 0)
                && (data == 0 || data & region != 0)
            {
                items
                    .try_reserve(1)
                    .map_err(|_| failure(NlsError32::Allocation))?;
                items.push(locale.lcid);
            }
        }
        if items.is_empty() {
            return Ok(WindowsProviderOutcome32::Return(1));
        }
        let continuation = self
            .nls
            .next_enumeration
            .checked_add(1)
            .ok_or(failure(NlsError32::Capacity))?;
        let thread = self.current_windows_thread_id()?;
        self.nls
            .enumerations
            .try_reserve(1)
            .map_err(|_| failure(NlsError32::Allocation))?;
        let bytes = Enumeration32::bytes(items[0], legacy).map_err(failure)?;
        let scratch = GuestCallbackScratch32::allocate(
            self.memory.as_mut().unwrap(),
            self.nls.scratch_reservation(),
            &bytes,
        )
        .map_err(HostError::Memory)?;
        let pending = Enumeration32 {
            continuation,
            thread,
            procedure: GuestAddress(args[0]),
            parameter: args[2],
            legacy,
            items,
            index: 0,
            scratch,
        };
        let request = pending.request();
        self.nls.enumerations.push(pending);
        self.nls.next_enumeration = continuation;
        Ok(WindowsProviderOutcome32::Callback(request))
    }
    pub(in crate::process) fn resume_nls_callback(
        &mut self,
        continuation: u64,
        result: u32,
    ) -> Result<WindowsProviderOutcome32, HostError> {
        self.memory()?;
        let thread = self.current_windows_thread_id()?;
        let slot = self
            .nls
            .enumerations
            .iter()
            .position(|pending| pending.continuation == continuation && pending.thread == thread)
            .ok_or(HostError::Nls(NlsError32::InvalidParameter))?;
        let pending = &self.nls.enumerations[slot];
        pending
            .scratch
            .check(self.memory.as_ref().unwrap())
            .map_err(HostError::Memory)?;
        if result == 0 || pending.index + 1 == pending.items.len() {
            pending
                .scratch
                .release(self.memory.as_mut().unwrap())
                .map_err(HostError::Memory)?;
            self.nls.enumerations.swap_remove(slot);
            return Ok(WindowsProviderOutcome32::Return(1));
        }
        let bytes = Enumeration32::bytes(pending.items[pending.index + 1], pending.legacy)
            .map_err(HostError::Nls)?;
        self.memory
            .as_mut()
            .unwrap()
            .write(pending.scratch.address, &bytes)
            .map_err(HostError::Memory)?;
        let pending = &mut self.nls.enumerations[slot];
        pending.index += 1;
        Ok(WindowsProviderOutcome32::Callback(pending.request()))
    }
}
