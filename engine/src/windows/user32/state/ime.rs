use super::*;
use crate::{memory::GuestAddress, windows::window::WindowExtent32};

impl User32State32 {
    pub(crate) fn default_ime_window(&self, thread: u32) -> Option<WindowHandle32> {
        self.slots
            .iter()
            .find(|slot| {
                slot.ime_window
                    && !slot.data.destroying
                    && self
                        .windows
                        .get(slot.data.handle)
                        .is_ok_and(|record| record.description.thread_id == thread)
            })
            .map(|slot| slot.data.handle)
    }

    pub(crate) fn is_ime_window(&self, window: WindowHandle32) -> Result<bool, User32Error32> {
        Ok(self.slots[self.slot(window)?].ime_window)
    }

    pub(crate) fn create_default_ime_window(
        &mut self,
        thread: u32,
        instance: u32,
        procedure: GuestAddress,
    ) -> Result<WindowHandle32, User32Error32> {
        if let Some(window) = self.default_ime_window(thread) {
            return Ok(window);
        }
        let description = WindowDescription32 {
            thread_id: thread,
            procedure,
            extent: WindowExtent32 {
                width: 0,
                height: 0,
            },
            visible: false,
            enabled: false,
        };
        let handle = self.check_create(description)?;
        let atom = self.install_system_class(WindowClass32 {
            name: vec![73, 77, 69],
            instance,
            procedure,
            unicode: true,
            style: 0x4000,
            ..Default::default()
        })?;
        let published = self.publish_window(
            description,
            WindowData32 {
                handle,
                class_atom: atom,
                instance,
                style: 0x8000_0000,
                ex_style: 0,
                user_data: 0,
                parent: None,
                menu: 0,
                outer: Default::default(),
                client_origin: Default::default(),
                unicode: true,
                destroying: false,
            },
            Box::new([]),
        );
        self.slots.last_mut().unwrap().ime_window = true;
        Ok(published)
    }
}
