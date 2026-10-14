use super::USER_MODE_ENVIRONMENT;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DescriptorTableRegister32 {
    pub base: u32,
    pub limit: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemRegisterQuery {
    Gdtr,
    Idtr,
    Ldtr,
    Tr,
    MachineStatusWord,
}

/// explicit readback images, separate from descriptor lookup and hidden system caches.
/// standalone banks may omit observations; process startup supplies complete defaults.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UserSystemRegisterBank32 {
    pub gdtr: Option<DescriptorTableRegister32>,
    pub idtr: Option<DescriptorTableRegister32>,
    pub ldtr: Option<u16>,
    pub tr: Option<u16>,
    pub cr0_low16: Option<u16>,
}

impl UserSystemRegisterBank32 {
    /// deterministic protected32 application observations. these opaque kernel
    /// addresses are readback values, not guest-memory tables or task authority.
    pub const fn application() -> Self {
        Self {
            gdtr: Some(DescriptorTableRegister32 {
                base: 0xffff_0000,
                limit: 0x7ff,
            }),
            idtr: Some(DescriptorTableRegister32 {
                base: 0xffff_1000,
                limit: 0x7ff,
            }),
            ldtr: Some(0),
            tr: Some(0x28),
            // pe/mp/et/ne set, em/ts clear: numeric state is available eagerly.
            cr0_low16: Some(0x33),
        }
    }

    pub(crate) fn with_application_defaults(self) -> Self {
        let application = Self::application();
        Self {
            gdtr: self.gdtr.or(application.gdtr),
            idtr: self.idtr.or(application.idtr),
            ldtr: self.ldtr.or(application.ldtr),
            tr: self.tr.or(application.tr),
            cr0_low16: self.cr0_low16.or(application.cr0_low16),
        }
    }

    pub(crate) fn is_valid(self) -> bool {
        !USER_MODE_ENVIRONMENT.user_mode_instruction_prevention()
            && self != Self::default()
            && self.ldtr.is_none_or(|selector| selector & 4 == 0)
            && self.tr.is_none_or(|selector| selector & 4 == 0)
            // PE/ET/NE are set; EM/TS/reserved low bits are clear. MP is caller-owned.
            // NE=0 would require FERR/IGNNE external error delivery instead of native FP faults.
            && self.cr0_low16.is_none_or(|value| value & 0xffcc == 0 && value & 0x31 == 0x31)
    }

    pub(crate) fn value(self, query: SystemRegisterQuery) -> Option<u16> {
        match query {
            SystemRegisterQuery::Ldtr => self.ldtr,
            SystemRegisterQuery::Tr => self.tr,
            SystemRegisterQuery::MachineStatusWord => self.cr0_low16,
            SystemRegisterQuery::Gdtr | SystemRegisterQuery::Idtr => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_presence_does_not_invent_values_or_validate_source_tables() {
        let empty = UserSystemRegisterBank32::default();
        assert!(!empty.is_valid());
        let bank = UserSystemRegisterBank32 {
            gdtr: Some(DescriptorTableRegister32 {
                base: u32::MAX,
                limit: 0,
            }),
            ldtr: Some(0),
            ..empty
        };
        assert!(bank.is_valid());
        assert_eq!(bank.value(SystemRegisterQuery::Ldtr), Some(0));
        assert_eq!(bank.value(SystemRegisterQuery::Tr), None);
        for value in [0x31, 0x33] {
            assert!(
                UserSystemRegisterBank32 {
                    cr0_low16: Some(value),
                    ..empty
                }
                .is_valid()
            );
        }
        for value in [0, 0x30, 0x35, 0x39, 0x21, 0x11, 0x71, 0xffff] {
            assert!(
                !UserSystemRegisterBank32 {
                    cr0_low16: Some(value),
                    ..empty
                }
                .is_valid()
            );
        }
        for selector in [0, 1, 2, 3, 0x40, 0x43, 0xfffb] {
            assert!(
                UserSystemRegisterBank32 {
                    tr: Some(selector),
                    ..empty
                }
                .is_valid()
            );
        }
        for selector in [4, 7, 0x44, 0xffff] {
            assert!(
                !UserSystemRegisterBank32 {
                    tr: Some(selector),
                    ..empty
                }
                .is_valid()
            );
        }
    }
}
