use crate::{
    process::{HostError, wasm::mutate},
    windows::{nls::NlsProfile32, text::CodePage32},
};

pub(crate) fn configure_nls(
    locale: u32,
    ansi: u32,
    oem: u32,
    bias: u32,
    max_text_units: u32,
) -> u32 {
    mutate(|instance| {
        let profile = NlsProfile32 {
            locale,
            ansi: CodePage32::from_id(ansi).ok_or(HostError::InvalidRequest)?,
            oem: CodePage32::from_id(oem).ok_or(HostError::InvalidRequest)?,
            time_zone_bias_minutes: bias as i32,
            max_text_units: max_text_units as usize,
        };
        instance.configure_nls(profile)
    })
}
