use crate::cpu::x86::UserSystemRegisterBank32;

/// immutable application environment used during semantic admission.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DecodeAuthority {
    pub(crate) system_registers: Option<UserSystemRegisterBank32>,
}

impl DecodeAuthority {
    pub(crate) const fn new(system_registers: Option<UserSystemRegisterBank32>) -> Self {
        Self { system_registers }
    }
}
