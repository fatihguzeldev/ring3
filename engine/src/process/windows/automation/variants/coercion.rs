use super::Coerced32;
use crate::{
    process::{EngineInstance, HostError},
    windows::automation::{
        AutomationError32 as E,
        variants::{Number32, NumberSymbols32, Variant32},
    },
};
impl EngineInstance {
    pub(in crate::process::windows::automation) fn coerce_automation_variant(
        &mut self,
        source: Variant32,
        locale: u32,
        flags: u16,
        target: u16,
    ) -> Result<Coerced32, HostError> {
        if target & 0xf000 != 0 {
            return Err(HostError::Automation(E::BadVariantType));
        }
        Variant32::new(target, &[])
            .validate()
            .map_err(HostError::Automation)?;
        if source.vartype() == target {
            return Ok(Coerced32::Value(source));
        }
        if source.array()
            || source.by_ref()
            || source.base() == 10
            || source.base() == 1 && target != 1
            || matches!(target, 9 | 13)
        {
            return Err(HostError::Automation(E::TypeMismatch));
        }
        if target == 0 || target == 1 {
            return Ok(Coerced32::Value(Variant32::new(target, &[])));
        }
        if target == 8 {
            if source.base() == 0 {
                return Ok(Coerced32::String(Vec::new()));
            }
            if source.base() == 7 {
                return self
                    .automation_date_text(source, locale, flags)
                    .map(Coerced32::String);
            }
            let text = if source.base() == 11 && flags & 0x12 != 0 {
                let truth = i16::from_le_bytes(source.0[8..10].try_into().unwrap()) != 0;
                if flags & 0x10 != 0 {
                    let locale = self.automation_locale_information(locale, 1)?.1.unwrap();
                    crate::windows::automation::variants::localized_boolean(locale, truth)
                        .to_owned()
                } else {
                    if truth { "True" } else { "False" }.to_owned()
                }
            } else {
                source
                    .number()
                    .map_err(HostError::Automation)?
                    .text()
                    .map_err(HostError::Automation)?
            };
            let decimal = self.automation_locale_information(locale, 0xe)?.0;
            let text = if decimal == "." {
                text
            } else {
                text.replace('.', &decimal)
            };
            return Ok(Coerced32::String(
                text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
            ));
        }
        if let Some(value) = source.boolean_integer(target) {
            return Ok(Coerced32::Value(value));
        }
        let number = if source.base() == 8 {
            let payload = self.read_automation_bstr(source.pointer())?;
            if payload.len() & 1 != 0 {
                return Err(HostError::Automation(E::TypeMismatch));
            }
            let units = payload
                .chunks_exact(2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>();
            if target == 7 {
                return self
                    .automation_parse_date(&units, locale, flags)
                    .map(Coerced32::Value);
            }
            if target == 11 {
                let key = crate::windows::text::ordinal_key(&units);
                if key == [84, 82, 85, 69] {
                    return Ok(Coerced32::Value(Variant32::new(11, &(-1i16).to_le_bytes())));
                }
                if key == [70, 65, 76, 83, 69] {
                    return Ok(Coerced32::Value(Variant32::new(11, &0i16.to_le_bytes())));
                }
                if flags & 0x10 != 0 {
                    let locale_id = self.automation_locale_information(locale, 1)?.1.unwrap();
                    for truth in [false, true] {
                        let word = crate::windows::automation::variants::localized_boolean(
                            locale_id, truth,
                        );
                        let localized = crate::windows::text::ordinal_key(
                            &word.encode_utf16().collect::<Vec<_>>(),
                        );
                        if key == localized {
                            return Ok(Coerced32::Value(Variant32::new(
                                11,
                                &if truth { -1i16 } else { 0i16 }.to_le_bytes(),
                            )));
                        }
                    }
                }
            }
            let information =
                |engine: &mut EngineInstance, kind: u32| -> Result<String, HostError> {
                    engine
                        .automation_locale_information(locale, kind)
                        .map(|value| value.0)
                };
            let symbols = NumberSymbols32 {
                decimal: information(self, 0xe)?,
                group: information(self, 0xf)?,
                negative: information(self, 0x51)?,
                currency: information(self, 0x14)?,
            };
            Number32::parse(&units, &symbols).map_err(HostError::Automation)?
        } else {
            source.number().map_err(HostError::Automation)?
        };
        number
            .convert(target)
            .map(Coerced32::Value)
            .map_err(HostError::Automation)
    }
}

impl EngineInstance {
    pub(super) fn automation_locale_information(
        &mut self,
        locale: u32,
        kind: u32,
    ) -> Result<(String, Option<u32>), HostError> {
        self.windows_locale_information(locale, kind)
            .map_err(locale_error)
    }
}

pub(super) fn locale_error(error: HostError) -> HostError {
    match error {
        HostError::Nls(error) => HostError::Automation(match error {
            crate::windows::nls::NlsError32::Allocation => E::Allocation,
            crate::windows::nls::NlsError32::Capacity => E::Capacity,
            crate::windows::nls::NlsError32::Memory(error) => E::Memory(error),
            _ => E::InvalidParameter,
        }),
        error => error,
    }
}
