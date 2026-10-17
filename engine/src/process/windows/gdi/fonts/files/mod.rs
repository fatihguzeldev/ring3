mod loading;
mod ownership;
mod types;
pub(in crate::process::windows::gdi) use types::{FileFontCall32, FileFontSource32};

use crate::{
    process::{
        EngineInstance, HostError, calls::call::PendingOwner, windows::files::ProviderFile32,
    },
    windows::gdi::GdiError32,
};
impl EngineInstance {
    pub(in crate::process) fn check_cancel_gdi_font_file(
        &self,
        owner: PendingOwner,
        token: u32,
    ) -> Result<Option<ProviderFile32>, HostError> {
        let call = self.gdi.as_ref().and_then(|gdi| {
            gdi.font_file_calls
                .iter()
                .find(|call| call.owner == owner && call.token == token)
        });
        let Some(call) = call else {
            return Ok(None);
        };
        self.check_close_provider_files(&[call.lease])?;
        Ok(Some(call.lease))
    }
    pub(in crate::process) fn commit_cancel_gdi_font_file(
        &mut self,
        source: Option<ProviderFile32>,
    ) {
        let Some(source) = source else {
            return;
        };
        self.commit_close_provider_files(&[source]);
        self.gdi
            .as_mut()
            .unwrap()
            .font_file_calls
            .retain(|call| call.lease != source);
    }
    fn font_file_path(&self, raw: u32, wide: bool) -> Result<Vec<u16>, HostError> {
        let input = crate::process::windows::strings::Input32::read(
            self.memory()?,
            raw,
            wide,
            self.windows_ansi_code_page(),
            32768,
        )
        .map_err(|error| match error {
            crate::process::windows::strings::WindowsStringError32::Memory(error) => {
                HostError::Memory(error)
            }
            crate::process::windows::strings::WindowsStringError32::Allocation => {
                HostError::Gdi(GdiError32::Allocation)
            }
            _ => HostError::Gdi(GdiError32::InvalidRequest),
        })?;
        if input.units.is_empty() || input.units.contains(&(b'|' as u16)) {
            return Err(HostError::Gdi(GdiError32::Unsupported));
        }
        self.provider_full_path(&input.units)
    }
}
