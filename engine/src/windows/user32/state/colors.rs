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
                        self.color_window_visible(slot.data.handle)
                    } else {
                        slot.data.style & 0x40000000 == 0
                    }
            })
            .map(|slot| slot.data.handle)
    }

    fn color_window_visible(&self, mut window: WindowHandle32) -> bool {
        for _ in 0..=self.slots.len() {
            let Ok(record) = self.windows.get(window) else {
                return false;
            };
            if !record.description.visible {
                return false;
            }
            let Ok(data) = self.record(window) else {
                return false;
            };
            if data.style & 0x40000000 == 0 {
                return true;
            }
            let Some(parent) = data.parent else {
                return false;
            };
            window = parent;
        }
        false
    }
}
