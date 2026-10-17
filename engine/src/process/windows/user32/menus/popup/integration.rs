use super::types::Stage32;
use crate::{
    process::windows::user32::{User32Context32, User32Outcome32, callback::Continuation32},
    windows::user32::User32Error32 as E,
};

impl User32Context32 {
    pub(in crate::process::windows::user32) fn popup_menu_item_rect(
        &self,
        menu: crate::windows::user32::menus::MenuHandle32,
        index: usize,
    ) -> Result<crate::windows::user32::Rect32, E> {
        self.menu_popups
            .plans
            .iter()
            .find_map(|plan| {
                plan.windows.iter().find_map(|popup| {
                    if popup.menu != menu {
                        return None;
                    }
                    self.windows().get(popup.handle?).ok()?;
                    popup
                        .visual
                        .as_ref()?
                        .layout
                        .screen_item(index, popup.origin)
                        .ok()
                })
            })
            .ok_or(E::Win32(1401))
    }

    pub(in crate::process::windows::user32) fn retry_popup_completion(
        &self,
        token: u64,
    ) -> Option<Result<User32Outcome32, E>> {
        let pending = self.pending.last()?;
        let Continuation32::MenuPopup { plan, .. } = pending.continuation else {
            return None;
        };
        let plan = self
            .menu_popups
            .plans
            .iter()
            .find(|entry| entry.token == plan)?;
        (plan.child.is_none() && plan.completed == Some(token)).then_some(Ok(
            User32Outcome32::Wait {
                continuation: plan.token,
            },
        ))
    }

    pub(in crate::process::windows::user32) fn preview_popup_input_flags(
        &self,
        token: u64,
    ) -> Result<Option<(bool, bool)>, E> {
        let pending = self
            .pending
            .last()
            .filter(|pending| pending.token == token)
            .ok_or(E::InvalidToken)?;
        let Continuation32::MenuPopup { plan: token, .. } = pending.continuation else {
            return Ok(None);
        };
        let plan = &self.menu_popups.plans[self.popup_index(token)?];
        let focus = self.windows().focus();
        let capture = self.windows().capture();
        match plan.stage {
            Stage32::Capture if !plan.cancelled => {
                let target = plan.windows.first().and_then(|popup| popup.handle);
                self.windows()
                    .check_set_capture(plan.call.thread_id, target)?;
                Ok(Some((focus.is_some(), target.is_some())))
            }
            Stage32::Destroy(depth) => {
                let Some(window) = plan.windows[depth].handle else {
                    return Ok(None);
                };
                let target = self.destroy_focus_target(window);
                self.windows()
                    .check_set_focus(plan.call.thread_id, target)?;
                Ok(Some((
                    target.is_some(),
                    capture
                        .is_some_and(|capture| !self.state.destroy_tree_contains(capture, window)),
                )))
            }
            Stage32::Release if plan.captured => Ok(Some((
                focus.is_some(),
                plan.restore_capture
                    .is_some_and(|window| self.windows().get(window).is_ok()),
            ))),
            _ => Ok(None),
        }
    }

    pub(in crate::process::windows::user32) fn retire_popup_thread(&mut self, thread: u32) {
        self.menu_popups
            .plans
            .retain(|plan| plan.call.thread_id != thread);
    }
}

impl crate::process::EngineInstance {
    pub(in crate::process::windows::user32) fn bind_popup_class(
        &mut self,
    ) -> Result<(), crate::process::HostError> {
        if self
            .user32
            .as_ref()
            .is_none_or(|context| context.menu_popups.procedure.is_some())
        {
            return Ok(());
        }
        let procedure = self
            .loaded_windows_modules()
            .flat_map(|module| module.gates())
            .find(|gate| gate.id == crate::windows::user32::User32Api32::DefWindowProcW.id())
            .map(|gate| gate.entry)
            .ok_or(crate::process::HostError::InvalidRequest)?;
        self.user32
            .as_mut()
            .unwrap()
            .install_popup_class(procedure)
            .map_err(crate::process::HostError::User32)
    }
}
