use super::*;
use crate::windows::user32::dialogs::WindowDialog32;

impl User32State32 {
    pub(crate) fn window_context_help(&self, window: WindowHandle32) -> Result<u32, User32Error32> {
        Ok(self.slots[self.slot(window)?].context_help)
    }
    pub(crate) fn set_window_context_help(
        &mut self,
        window: WindowHandle32,
        help: u32,
    ) -> Result<(), User32Error32> {
        let index = self.slot(window)?;
        self.slots[index].context_help = help;
        Ok(())
    }

    pub(crate) fn window_limit(&self) -> usize {
        self.limits.windows
    }

    pub(crate) fn check_window_capacity(&self, additional: usize) -> Result<(), User32Error32> {
        if additional > self.limits.windows.saturating_sub(self.slots.len()) {
            return Err(User32Error32::Capacity);
        }
        Ok(())
    }

    pub(crate) fn dialog(&self, window: WindowHandle32) -> Result<WindowDialog32, User32Error32> {
        self.slots[self.slot(window)?]
            .dialog
            .ok_or(User32Error32::Win32(1420))
    }

    pub(crate) fn set_dialog(
        &mut self,
        window: WindowHandle32,
        dialog: WindowDialog32,
    ) -> Result<(), User32Error32> {
        let index = self.slot(window)?;
        self.slots[index].dialog = Some(dialog);
        Ok(())
    }

    pub(crate) fn dialog_item(
        &self,
        window: WindowHandle32,
        id: u32,
    ) -> Result<Option<WindowHandle32>, User32Error32> {
        self.windows().get(window)?;
        Ok(self
            .slots
            .iter()
            .find(|slot| {
                slot.data.parent == Some(window)
                    && slot.data.style & 0x4000_0000 != 0
                    && slot.data.menu == id
            })
            .map(|slot| slot.data.handle))
    }

    pub(crate) fn dialog_tab_item(
        &self,
        window: WindowHandle32,
        previous: Option<WindowHandle32>,
        backwards: bool,
    ) -> Result<Option<WindowHandle32>, User32Error32> {
        self.windows().get(window)?;
        let eligible = |slot: &&WindowSlot32| {
            slot.data.parent == Some(window)
                && slot.data.style & (0x4000_0000 | 0x1000_0000 | 0x0800_0000 | 0x10000)
                    == 0x5001_0000
                && !slot.data.destroying
        };
        let windows = self
            .slots
            .iter()
            .filter(eligible)
            .map(|slot| slot.data.handle);
        if backwards {
            let last = windows.clone().next_back();
            Ok(match previous {
                Some(previous) => windows
                    .take_while(|&window| window != previous)
                    .last()
                    .or(last),
                None => last,
            })
        } else {
            let first = windows.clone().next();
            Ok(match previous {
                Some(previous) => windows
                    .skip_while(|&window| window != previous)
                    .nth(1)
                    .or(first),
                None => first,
            })
        }
    }
}
