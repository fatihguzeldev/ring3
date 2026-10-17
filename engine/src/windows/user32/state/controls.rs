use super::*;
use crate::windows::{controls::ControlKind32, gdi::Region32};

impl User32State32 {
    pub(crate) fn check_control_changes(&self, count: u64) -> Result<(), User32Error32> {
        self.queue.check_external_changes(count)
    }
    pub(crate) fn invalidate_control(
        &mut self,
        window: WindowHandle32,
    ) -> Result<(), User32Error32> {
        let extent = self.windows.get(window)?.description.extent;
        self.set_update_region(
            window,
            Region32::rectangle(super::super::Rect32 {
                left: 0,
                top: 0,
                right: extent.width as i32,
                bottom: extent.height as i32,
            }),
            true,
        )
    }
    pub(crate) fn control_list_owner(
        &self,
        window: WindowHandle32,
    ) -> Result<Option<WindowHandle32>, User32Error32> {
        Ok(self.slots[self.slot(window)?].control.list_owner)
    }
    pub(crate) fn set_control_list_owner(&mut self, window: WindowHandle32, owner: WindowHandle32) {
        let index = self.slot(window).expect("new list window owns a slot");
        self.slots[index].control.list_owner = Some(owner);
    }
    pub(crate) fn control_radio_group(
        &self,
        window: WindowHandle32,
    ) -> Result<Vec<WindowHandle32>, User32Error32> {
        let data = self.record(window)?;
        let mut group = Vec::new();
        group
            .try_reserve(self.slots.len())
            .map_err(|_| User32Error32::Allocation)?;
        for slot in &self.slots {
            if slot.data.parent != data.parent || slot.data.style & 0x4000_0000 == 0 {
                continue;
            }
            if slot.data.style & 0x0002_0000 != 0 {
                if group.contains(&window) {
                    break;
                }
                group.clear();
            }
            group.push(slot.data.handle);
        }
        group.retain(|handle| {
            self.record(*handle).is_ok_and(|record| {
                self.standard_kind(record.class_atom) == Some(ControlKind32::Button)
                    && matches!(record.style & 15, 4 | 9)
            })
        });
        Ok(group)
    }
    pub(crate) fn control_mouse_target(
        &self,
        root: WindowHandle32,
        point: Point32,
    ) -> Option<WindowHandle32> {
        for slot in self.slots.iter().rev() {
            let data = slot.data;
            if let Some(owner) = slot.control.list_owner {
                if !data.destroying
                    && contains(data.outer, point)
                    && self.top_level(owner).ok() == Some(root)
                    && self.windows.get(data.handle).is_ok_and(|record| {
                        record.description.visible && record.description.enabled
                    })
                    && self.windows.get(owner).is_ok_and(|record| {
                        record.description.visible && record.description.enabled
                    })
                {
                    return Some(data.handle);
                }
                continue;
            }
            if data.destroying
                || data.handle == root
                || data.style & 0x4000_0000 == 0
                || !contains(data.outer, point)
                || self.standard_kind(data.class_atom) == Some(ControlKind32::Button)
                    && data.style & 15 == 7
            {
                continue;
            }
            let mut candidate = Some(data.handle);
            for _ in 0..self.slots.len() {
                let handle = candidate?;
                let Ok(record) = self.windows.get(handle) else {
                    break;
                };
                if !record.description.visible || !record.description.enabled {
                    break;
                }
                if handle == root {
                    return Some(data.handle);
                }
                let Ok(ancestor) = self.record(handle) else {
                    break;
                };
                if !contains(ancestor.outer, point) {
                    break;
                }
                candidate = ancestor.parent;
                if candidate.is_none() {
                    break;
                }
            }
        }
        None
    }
}
fn contains(rect: super::super::Rect32, point: Point32) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}
