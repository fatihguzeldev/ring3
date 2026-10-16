use crate::{
    process::{EngineInstance, HostError},
    windows::automation::{AutomationError32 as E, AutomationLimits32},
};
#[derive(Clone, Copy)]
pub(super) struct Bstr32 {
    pub pointer: u32,
    pub bytes: usize,
}
pub(in crate::process) struct AutomationRuntime32 {
    pub(super) limits: AutomationLimits32,
    pub(super) strings: Vec<Bstr32>,
    pub(super) bytes: usize,
}
impl Default for AutomationRuntime32 {
    fn default() -> Self {
        Self {
            limits: AutomationLimits32::default(),
            strings: Vec::new(),
            bytes: 0,
        }
    }
}
impl AutomationRuntime32 {
    pub(in crate::process) fn terminate(&mut self) {
        self.strings.clear();
        self.bytes = 0;
    }
    pub(super) fn prepare_string(&mut self, bytes: usize, replace: Option<usize>) -> Result<(), E> {
        let old = replace.map_or(0, |i| self.strings[i].bytes);
        if (replace.is_none() && self.strings.len() >= self.limits.objects)
            || self
                .bytes
                .checked_sub(old)
                .and_then(|n| n.checked_add(bytes))
                .is_none_or(|n| n > self.limits.bytes)
        {
            return Err(E::Capacity);
        }
        self.strings
            .try_reserve(usize::from(replace.is_none()))
            .map_err(|_| E::Allocation)
    }
}
impl EngineInstance {
    pub fn configure_automation(&mut self, limits: AutomationLimits32) -> Result<(), HostError> {
        self.memory()?;
        if self.image_started
            || self.pending_call.is_some()
            || self.callback.is_some()
            || !self.automation.strings.is_empty()
        {
            return Err(HostError::InvalidRequest);
        }
        limits.validate().map_err(HostError::Automation)?;
        self.automation = AutomationRuntime32 {
            limits,
            ..Default::default()
        };
        Ok(())
    }
}
