/// trusted operating-system authority for user-mode port access.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UserPortPermissions {
    /// every port is denied without a guest-memory tss or bitmap lookup.
    DenyAll,
}

/// immutable operating-system guarantees of the fixed protected32 user-mode profile.
/// these are execution controls, separate from cpuid instruction availability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UserModeEnvironment {
    current_privilege: u8,
    user_mode_instruction_prevention: bool,
    io_privilege: u8,
    user_port_permissions: UserPortPermissions,
    performance_counter_access: bool,
    tsc_aux: u32,
    protected_virtual_interrupts: bool,
    debug_general_detect: bool,
    debug_extensions_enabled: bool,
}

pub(crate) const USER_MODE_ENVIRONMENT: UserModeEnvironment = UserModeEnvironment {
    current_privilege: 3,
    user_mode_instruction_prevention: false,
    io_privilege: 0,
    user_port_permissions: UserPortPermissions::DenyAll,
    performance_counter_access: false,
    tsc_aux: 0,
    protected_virtual_interrupts: false,
    debug_general_detect: false,
    debug_extensions_enabled: false,
};

impl UserModeEnvironment {
    pub(crate) const fn user_mode_instruction_prevention(self) -> bool {
        self.user_mode_instruction_prevention
    }

    pub(crate) const fn current_privilege(self) -> u8 {
        self.current_privilege
    }

    pub(crate) const fn io_privilege(self) -> u8 {
        self.io_privilege
    }

    pub(crate) const fn user_port_permissions(self) -> UserPortPermissions {
        self.user_port_permissions
    }

    /// cr4.pce permits performance-counter reads outside privilege level zero.
    pub(crate) const fn performance_counter_access(self) -> bool {
        self.performance_counter_access
    }

    /// identity of the single virtual processor, independent of timestamp sampling.
    pub(crate) const fn tsc_aux(self) -> u32 {
        self.tsc_aux
    }

    pub(crate) const fn protected_virtual_interrupts(self) -> bool {
        self.protected_virtual_interrupts
    }

    pub(crate) const fn debug_general_detect(self) -> bool {
        self.debug_general_detect
    }

    pub(crate) const fn debug_extensions_enabled(self) -> bool {
        self.debug_extensions_enabled
    }
}

#[cfg(test)]
mod tests {
    use super::{USER_MODE_ENVIRONMENT, UserPortPermissions};

    #[test]
    fn user_environment_keeps_privilege_interrupt_and_debug_controls_explicit() {
        assert_eq!(USER_MODE_ENVIRONMENT.current_privilege(), 3);
        assert!(!USER_MODE_ENVIRONMENT.user_mode_instruction_prevention());
        assert_eq!(USER_MODE_ENVIRONMENT.io_privilege(), 0);
        assert_eq!(
            USER_MODE_ENVIRONMENT.user_port_permissions(),
            UserPortPermissions::DenyAll,
        );
        assert!(!USER_MODE_ENVIRONMENT.performance_counter_access());
        assert_eq!(USER_MODE_ENVIRONMENT.tsc_aux(), 0);
        assert!(!USER_MODE_ENVIRONMENT.protected_virtual_interrupts());
        assert!(!USER_MODE_ENVIRONMENT.debug_general_detect());
        assert!(!USER_MODE_ENVIRONMENT.debug_extensions_enabled());
    }
}
