use crate::cpu::x86::UserSystemRegisterBank32;

/// explicit immutable inputs to semantic admission, separate from code-fetch context.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DecodeAuthority {
    pub(crate) system_registers: Option<UserSystemRegisterBank32>,
    pub(crate) interrupts_configured: bool,
    pub(crate) task_return_configured: bool,
    pub(crate) task_execution_revision: Option<u64>,
    pub(crate) coprocessor_cr0: Option<u32>,
}

impl DecodeAuthority {
    pub(crate) const fn new(
        system_registers: Option<UserSystemRegisterBank32>,
        interrupts_configured: bool,
    ) -> Self {
        Self {
            system_registers,
            interrupts_configured,
            task_return_configured: false,
            task_execution_revision: None,
            coprocessor_cr0: None,
        }
    }

    pub(crate) const fn with_coprocessor_control(mut self, cr0: Option<u32>) -> Self {
        self.coprocessor_cr0 = cr0;
        self
    }

    pub(crate) const fn with_task_revision(mut self, revision: Option<u64>) -> Self {
        self.task_execution_revision = revision;
        self
    }

    pub(crate) const fn with_task_return(mut self, configured: bool) -> Self {
        self.task_return_configured = configured;
        self
    }
}
