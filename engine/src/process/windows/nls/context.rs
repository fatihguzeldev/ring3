use crate::windows::nls::{NlsError32, NlsProfile32};
#[derive(Default)]
pub(in crate::process) struct NlsRuntime32 {
    pub(super) profile: NlsProfile32,
    pub(super) locales: Vec<(u32, u32, u16)>,
    pub(super) enumerations: Vec<super::enumeration::Enumeration32>,
    pub(super) next_enumeration: u64,
    data: Vec<(u32, crate::windows::nls::locales::LocaleData32)>,
}
impl NlsRuntime32 {
    pub(super) fn data(
        &mut self,
        locale: u32,
    ) -> Result<&crate::windows::nls::locales::LocaleData32, NlsError32> {
        if let Some(index) = self.data.iter().position(|entry| entry.0 == locale) {
            return Ok(&self.data[index].1);
        }
        let data = crate::windows::nls::locales::LocaleData32::new(locale, self.profile.locale)?;
        self.data
            .try_reserve(1)
            .map_err(|_| NlsError32::Allocation)?;
        if self.data.len() == 16 {
            self.data.remove(0);
        }
        self.data.push((locale, data));
        Ok(&self.data.last().unwrap().1)
    }
    pub(super) fn locale_code_page(
        &self,
        locale: u32,
        flags: u32,
    ) -> Result<crate::windows::text::CodePage32, NlsError32> {
        let ansi = crate::windows::nls::locales::locale_identity(locale)?.ansi;
        if flags & 0x40000000 != 0 || ansi == 0 {
            return Ok(self.profile.ansi);
        }
        crate::windows::text::CodePage32::from_id(ansi).ok_or(NlsError32::InvalidParameter)
    }
    pub(super) fn code_page(
        &self,
        id: u32,
        thread: u32,
    ) -> Result<crate::windows::text::CodePage32, NlsError32> {
        if id == 3 {
            let ansi = crate::windows::nls::locales::locale_identity(self.locale(thread))?.ansi;
            return crate::windows::text::CodePage32::from_id(if ansi == 0 {
                self.profile.ansi.id()
            } else {
                ansi
            })
            .ok_or(NlsError32::InvalidParameter);
        }
        self.profile.code_page(id)
    }
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
        self.enumerations.retain(|pending| pending.thread != thread);
    }
}
impl crate::process::EngineInstance {
    pub(in crate::process) fn windows_user_locale(&self) -> u32 {
        self.nls.profile.locale
    }
    pub(in crate::process) fn windows_thread_locale(&self) -> u32 {
        self.nls.locale(self.threads.current_id())
    }
    pub(in crate::process) fn windows_ansi_code_page(&self) -> crate::windows::text::CodePage32 {
        self.nls.profile.ansi
    }
    pub(in crate::process) fn windows_oem_code_page(&self) -> crate::windows::text::CodePage32 {
        self.nls.profile.oem
    }
    pub(in crate::process) fn current_thread_ui_language(&self) -> u16 {
        self.nls.language(self.threads.current_id())
    }
    pub fn configure_nls(
        &mut self,
        profile: NlsProfile32,
    ) -> Result<(), crate::process::HostError> {
        self.memory()?;
        if self.image.is_some()
            || self.module_teb_address().is_some()
            || self.image_started
            || self.pending_call.is_some()
            || self.callback.is_some()
        {
            return Err(crate::process::HostError::InvalidRequest);
        }
        profile.validate().map_err(crate::process::HostError::Nls)?;
        self.nls = NlsRuntime32 {
            profile,
            locales: Vec::new(),
            data: Vec::new(),
            enumerations: Vec::new(),
            next_enumeration: 0,
        };
        Ok(())
    }
}
