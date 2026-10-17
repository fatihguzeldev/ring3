use super::*;

impl User32State32 {
    pub(crate) fn color_window_boundary(&self) -> u32 {
        self.slots.last().map_or(0, |slot| slot.data.handle.get())
    }

    pub(crate) fn next_color_window(
        &self,
        after: u32,
        through: u32,
        repaint: bool,
    ) -> Option<WindowHandle32> {
        let first = self
            .slots
            .partition_point(|slot| slot.data.handle.get() <= after);
        self.slots[first..]
            .iter()
            .take_while(|slot| slot.data.handle.get() <= through)
            .find(|slot| {
                !slot.data.destroying
                    && if repaint {
                        self.window_visible(slot.data.handle)
                    } else {
                        slot.data.style & 0x40000000 == 0
                    }
            })
            .map(|slot| slot.data.handle)
    }
}
