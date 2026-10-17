use super::*;

impl User32State32 {
    pub(crate) fn ui_state(&self, window: WindowHandle32) -> Result<u32, User32Error32> {
        Ok(self.slots[self.slot(window)?].ui_state)
    }

    pub(crate) fn set_ui_state(
        &mut self,
        window: WindowHandle32,
        value: u32,
    ) -> Result<(), User32Error32> {
        let index = self.slot(window)?;
        self.slots[index].ui_state = value;
        Ok(())
    }

    pub(crate) fn ui_child_boundary(&self, parent: WindowHandle32) -> u32 {
        self.related_windows(parent, true)
            .next_back()
            .map_or(0, WindowHandle32::get)
    }

    pub(crate) fn next_ui_child(
        &self,
        parent: WindowHandle32,
        after: u32,
        through: u32,
    ) -> Option<WindowHandle32> {
        self.related_windows(parent, true).find(|&child| {
            child.get() > after
                && child.get() <= through
                && self.record(child).is_ok_and(|data| !data.destroying)
        })
    }
}
