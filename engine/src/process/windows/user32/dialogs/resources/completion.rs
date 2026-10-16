use crate::{
    memory::AddressSpace,
    process::windows::user32::{User32Context32, User32Outcome32, callback::Continuation32},
    windows::user32::User32Error32 as E,
};

impl User32Context32 {
    pub(in crate::process::windows::user32) fn check_resource_dialog_scratch(
        &self,
        ram: &AddressSpace,
    ) -> Result<(), E> {
        for pending in &self.pending {
            if let Continuation32::ResourceDialog { scratch, .. } = pending.continuation {
                scratch.check(ram)?;
            }
        }
        Ok(())
    }

    pub(in crate::process::windows::user32) fn retry_resource_dialog_completion(
        &mut self,
        token: u64,
        ram: &mut AddressSpace,
    ) -> Option<Result<User32Outcome32, E>> {
        let pending = self.pending.last()?;
        let Continuation32::ResourceDialog {
            plan,
            child: None,
            completed: Some(completed),
            ..
        } = pending.continuation
        else {
            return None;
        };
        (completed == token).then(|| self.advance_resource_dialog(plan, ram))
    }

    pub(in crate::process::windows::user32) fn complete_resource_dialog_child(
        &mut self,
        token: u64,
        outcome: User32Outcome32,
        ram: &mut AddressSpace,
    ) -> Result<User32Outcome32, E> {
        if let User32Outcome32::Return(result) = outcome
            && let Some(pending) = self.pending.last()
            && let Continuation32::ResourceDialog {
                plan,
                child: Some(child),
                ..
            } = pending.continuation
            && child == token
        {
            return self.accept_resource_delegate(plan, token, result, ram);
        }
        Ok(outcome)
    }
}
