use crate::cpu::x86::UserSystemRegisterBank32;

/// explicit immutable inputs to semantic admission, separate from code-fetch context.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DecodeAuthority {
    pub(crate) system_registers: Option<UserSystemRegisterBank32>,
    pub(crate) interrupts_configured: bool,
}

impl DecodeAuthority {
    pub(crate) const fn new(
        system_registers: Option<UserSystemRegisterBank32>,
        interrupts_configured: bool,
    ) -> Self {
        Self {
            system_registers,
            interrupts_configured,
        }
    }
}
