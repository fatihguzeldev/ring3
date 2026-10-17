use super::{Continuation32, User32Context32, User32Outcome32};
use crate::{
    memory::AddressSpace,
    process::{HostError, windows::gdi::GdiRuntime32},
    windows::user32::User32Error32 as E,
};

impl User32Context32 {
    pub(in crate::process) fn preview_wait_input_flags(
        &self,
        token: u64,
    ) -> Result<Option<(bool, bool)>, E> {
        if let Some(flags) = self.preview_tree_input_flags(token)? {
            return Ok(Some(flags));
        }
        if let Some(flags) = self.preview_numeric_input_flags(token)? {
            return Ok(Some(flags));
        }
        if let Some(flags) = self.preview_listview_input_flags(token)? {
            return Ok(Some(flags));
        }
        if let Some(flags) = self.preview_header_input_flags(token)? {
            return Ok(Some(flags));
        }
        if let Some(flags) = self.preview_tab_selection_input_flags(token)? {
            return Ok(Some(flags));
        }
        if matches!(
            self.pending.last().map(|pending| pending.continuation),
            Some(Continuation32::MenuPopup { .. })
        ) {
            self.preview_popup_input_flags(token)
        } else {
            self.preview_resource_dialog_wait_input_flags(token)
        }
    }

    pub(in crate::process) fn resume_wait_with_gdi(
        &mut self,
        token: u64,
        ram: &mut AddressSpace,
        gdi: Option<&mut GdiRuntime32>,
    ) -> Result<User32Outcome32, E> {
        let popup = self
            .pending
            .last()
            .filter(|pending| pending.token == token)
            .is_some_and(|pending| {
                matches!(pending.continuation, Continuation32::MenuPopup { .. })
            });
        if self.pending.last().is_some_and(|pending| {
            pending.token == token && matches!(pending.continuation, Continuation32::StatusDraw(_))
        }) {
            return self
                .resume_status_draw(token, ram, gdi.ok_or(E::Win32(50))?)
                .map_err(user32_error);
        }
        if self.tree_change_wait(token) {
            let outcome =
                self.resume_tree_change(token, ram, gdi.as_deref().ok_or(E::Win32(50))?)?;
            return self.complete_user32_child(token, outcome, ram);
        }
        if self.tab_tooltip_wait(token) {
            let outcome = self.resume_tab_tooltip(token, ram)?;
            return self.complete_user32_child(token, outcome, ram);
        }
        if self.tooltip_text_wait(token) {
            let outcome = self
                .resume_tooltip_text(token, ram, gdi.ok_or(E::Win32(50))?)
                .map_err(user32_error)?;
            return self.complete_user32_child(token, outcome, ram);
        }
        if self.tooltip_relay_wait(token) {
            let outcome = self
                .resume_tooltip_relay(token, ram, gdi.ok_or(E::Win32(50))?)
                .map_err(user32_error)?;
            return self.complete_user32_child(token, outcome, ram);
        }
        if self.tooltip_tracking_wait(token) {
            let outcome = self
                .resume_tooltip_tracking(token, ram, gdi.ok_or(E::Win32(50))?)
                .map_err(user32_error)?;
            return self.complete_user32_child(token, outcome, ram);
        }
        if self.menu_bar_wait(token).is_some() {
            return self.resume_menu_bar(token, ram, gdi.ok_or(E::Win32(50))?);
        }
        if popup {
            self.resume_popup_menu(token, ram, gdi)
        } else {
            self.resume_wait(token, ram)
        }
    }
}

fn user32_error(error: HostError) -> E {
    match error {
        HostError::User32(error) => error,
        HostError::Memory(error) => E::Memory(error),
        HostError::Gdi(error) => crate::process::windows::user32::menus::gdi_error(error),
        _ => E::InvalidRequest,
    }
}
