use super::User32State32;
use crate::windows::user32::menus::MenuHandle32;

impl User32State32 {
    pub(crate) fn attached_menus(&self, thread: u32) -> impl Iterator<Item = MenuHandle32> + '_ {
        self.slots.iter().filter_map(move |slot| {
            (slot.data.style & 0x4000_0000 == 0
                && self
                    .windows
                    .get(slot.data.handle)
                    .is_ok_and(|window| window.description.thread_id == thread))
            .then(|| MenuHandle32::try_from(slot.data.menu).ok())
            .flatten()
        })
    }
}
