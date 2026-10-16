use super::types::Stage32;
use crate::{
    process::windows::user32::{User32Context32, callback::Continuation32},
    windows::user32::User32Error32 as E,
};

impl User32Context32 {
    pub(in crate::process) fn preview_resource_dialog_wait_input_flags(
        &self,
        token: u64,
    ) -> Result<Option<(bool, bool)>, E> {
        let pending = self
            .pending
            .last()
            .filter(|pending| pending.token == token)
            .ok_or(E::InvalidToken)?;
        let Continuation32::ResourceDialog { plan, .. } = pending.continuation else {
            return Ok(None);
        };
        let plan = &self.resource_dialogs.plans[self.resource_dialog_index(plan)?];
        let root = plan.windows.first().copied();
        if root
            .and_then(|root| self.state.dialog(root).ok())
            .is_some_and(|dialog| dialog.ended.is_some())
            && !matches!(
                plan.stage,
                Stage32::Destroy | Stage32::Restore | Stage32::RestoreFocus
            )
        {
            // the resumed loop first selects destruction, then yields for that exact stage.
            return Ok(None);
        }
        let target = match plan.stage {
            Stage32::Focus => {
                let Some(root) = root else {
                    return Ok(None);
                };
                let first = self.state.dialog_tab_item(root, None, false)?;
                if first.is_none() || self.windows().focus() == first {
                    return Ok(None);
                }
                first
            }
            Stage32::Destroy => {
                let Some(root) = root.filter(|&root| self.windows().get(root).is_ok()) else {
                    return Ok(None);
                };
                self.destroy_focus_target(root)
            }
            Stage32::RestoreFocus => {
                let previous = plan.previous_focus.filter(|&window| {
                    self.windows()
                        .get(window)
                        .is_ok_and(|record| record.description.enabled)
                });
                if previous.is_none() || self.windows().focus() == previous {
                    return Ok(None);
                }
                previous
            }
            _ => return Ok(None),
        };
        self.windows().check_set_focus(plan.thread, target)?;
        Ok(Some((target.is_some(), self.windows().capture().is_some())))
    }
}
