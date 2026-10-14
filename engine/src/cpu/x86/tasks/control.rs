#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskControlError32 {
    UnsupportedControls,
    InvalidLdtr,
    RequiresDebugAuthority,
}

/// supplied live controls, separate from readback images and hidden tr ownership.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskControlState32 {
    cr0: u32,
    cr3: u32,
    ldtr: u16,
    dr7: u32,
    nmi_blocked: Option<bool>,
    cpl: u8,
}

impl TaskControlState32 {
    pub fn new(
        cr0: u32,
        cr3: u32,
        ldtr: u16,
        dr7: u32,
        nmi_blocked: Option<bool>,
    ) -> Result<Self, TaskControlError32> {
        // the initial protected user context supplies pe/et/ne, mp/ts and pg explicitly.
        if cr0 & 0x31 != 0x31 || cr0 & !0x8001_003f != 0 {
            return Err(TaskControlError32::UnsupportedControls);
        }
        if ldtr & 4 != 0 {
            return Err(TaskControlError32::InvalidLdtr);
        }
        if dr7 & 0x20ff != 0 {
            return Err(TaskControlError32::RequiresDebugAuthority);
        }
        Ok(Self {
            cr0,
            cr3,
            ldtr,
            dr7,
            nmi_blocked,
            cpl: 3,
        })
    }

    pub const fn cr0(self) -> u32 {
        self.cr0
    }
    pub const fn cr3(self) -> u32 {
        self.cr3
    }
    pub const fn ldtr(self) -> u16 {
        self.ldtr
    }
    pub const fn dr7(self) -> u32 {
        self.dr7
    }
    pub const fn nmi_blocked(self) -> Option<bool> {
        self.nmi_blocked
    }
    pub const fn current_privilege(self) -> u8 {
        self.cpl
    }
    pub const fn paging_enabled(self) -> bool {
        self.cr0 & 0x8000_0000 != 0
    }
    pub const fn task_switched(self) -> bool {
        self.cr0 & 8 != 0
    }
    pub const fn monitor_coprocessor(self) -> bool {
        self.cr0 & 2 != 0
    }

    pub(crate) fn after_return(self, ldtr: u16, cpl: u8) -> Self {
        Self {
            cr0: self.cr0 | 8,
            // with supplied pg0, incoming tss cr3 is read but not loaded.
            cr3: self.cr3,
            ldtr,
            dr7: self.dr7 & !0x55,
            nmi_blocked: self.nmi_blocked.map(|_| false),
            cpl,
        }
    }

    pub(crate) fn unblock_nmi(&mut self) {
        if self.nmi_blocked.is_some() {
            self.nmi_blocked = Some(false);
        }
    }
}

#[cfg(test)]
#[path = "control/tests.rs"]
mod tests;
