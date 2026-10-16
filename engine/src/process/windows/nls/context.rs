use crate::windows::nls::{NlsError32, NlsProfile32};
#[derive(Default)]
pub(in crate::process) struct NlsRuntime32 {
    pub(super) profile: NlsProfile32,
    locales: Vec<(u32, u32, u16)>,
}
impl NlsRuntime32 {
    pub(in crate::process) fn language(&self, thread: u32) -> u16 {
        self.locales
            .iter()
            .find(|entry| entry.0 == thread)
            .map_or(self.profile.locale as u16, |entry| entry.2)
    }
    pub(super) fn locale(&self, thread: u32) -> u32 {
        self.locales
            .iter()
            .find(|entry| entry.0 == thread)
            .map_or(self.profile.locale, |entry| entry.1)
    }
    pub(super) fn set(
        &mut self,
        thread: u32,
        locale: Option<u32>,
        language: Option<u16>,
    ) -> Result<(), NlsError32> {
        if let Some(entry) = self.locales.iter_mut().find(|entry| entry.0 == thread) {
            if let Some(value) = locale {
                entry.1 = value;
            }
            if let Some(value) = language {
                entry.2 = value;
            }
            return Ok(());
        }
        if self.locales.len() == 4096 {
            return Err(NlsError32::Capacity);
        }
        self.locales
            .try_reserve(1)
            .map_err(|_| NlsError32::Allocation)?;
        self.locales.push((
            thread,
            locale.unwrap_or(self.profile.locale),
            language.unwrap_or(self.profile.locale as u16),
        ));
        Ok(())
    }
    pub(in crate::process) fn retire_thread(&mut self, thread: u32) {
        self.locales.retain(|entry| entry.0 != thread);
    }
}
impl crate::process::EngineInstance {
    pub(in crate::process) fn current_thread_ui_language(&self) -> u16 {
        self.nls.language(self.threads.current_id())
    }
    pub fn configure_nls(
        &mut self,
        profile: NlsProfile32,
    ) -> Result<(), crate::process::HostError> {
        self.memory()?;
        if self.image_started || self.pending_call.is_some() || self.callback.is_some() {
            return Err(crate::process::HostError::InvalidRequest);
        }
        profile.validate().map_err(crate::process::HostError::Nls)?;
        self.nls = NlsRuntime32 {
            profile,
            locales: Vec::new(),
        };
        Ok(())
    }
}

impl crate::process::EngineInstance {
    pub(in crate::process) fn windows_oem_code_page(&self) -> crate::windows::text::CodePage32 {
        self.nls.profile.oem
    }
}
