use crate::{
    process::{
        HostError,
        windows::{gdi::GdiRuntime32, user32::User32Context32},
    },
    windows::gdi::GdiError32,
};

impl User32Context32 {
    pub(in crate::process) fn retire_resource_dialog_fonts(
        &mut self,
        gdi: &mut GdiRuntime32,
    ) -> Result<(), HostError> {
        gdi.retire_dead_windows(|raw| {
            crate::windows::window::WindowHandle32::try_from(raw)
                .ok()
                .is_some_and(|window| self.windows().get(window).is_ok())
        });
        let mut at = 0;
        while at < self.resource_dialogs.fonts.len() {
            let lease = &self.resource_dialogs.fonts[at];
            if lease
                .window
                .is_some_and(|window| self.windows().get(window).is_ok())
                || self.resource_dialogs.plans.iter().any(|plan| {
                    plan.font == lease.font
                        || plan
                            .prepared
                            .windows
                            .iter()
                            .any(|window| window.font == lease.font)
                })
            {
                at += 1;
                continue;
            }
            match gdi.retire_dialog_font(lease.font) {
                Ok(()) | Err(GdiError32::InvalidHandle) => {
                    self.resource_dialogs.fonts.remove(at);
                }
                Err(GdiError32::SelectedObject) => at += 1,
                Err(error) => return Err(HostError::Gdi(error)),
            }
        }
        Ok(())
    }

    pub(in crate::process::windows::user32) fn retire_resource_dialog_thread(
        &mut self,
        thread: u32,
    ) {
        self.resource_dialogs
            .plans
            .retain(|plan| plan.thread != thread);
    }
}
