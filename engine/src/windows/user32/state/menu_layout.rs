use super::*;
use crate::windows::user32::menus::{MenuHandle32, MenuLayout32};

#[derive(Clone, Copy)]
pub(crate) struct MenuOwnerSize32 {
    pub revision: u64,
    pub width: i32,
    pub height: i32,
}

pub(crate) struct MenuBarCache32 {
    pub menu: MenuHandle32,
    pub revision: u64,
    pub measurements: Vec<Option<MenuOwnerSize32>>,
    pub layout: Option<MenuLayout32>,
    pub origin: Point32,
    pub gutter: i32,
    pub padding: i32,
}

impl User32State32 {
    pub(crate) fn menu_bar_cache(
        &self,
        window: WindowHandle32,
    ) -> Result<Option<&MenuBarCache32>, User32Error32> {
        Ok(self.slots[self.slot(window)?].menu_bar.as_ref())
    }

    pub(crate) fn menu_bar_cache_mut(
        &mut self,
        window: WindowHandle32,
    ) -> Result<&mut MenuBarCache32, User32Error32> {
        let index = self.slot(window)?;
        self.slots[index]
            .menu_bar
            .as_mut()
            .ok_or(User32Error32::InvalidRequest)
    }

    pub(crate) fn prepare_menu_bar_cache(
        &mut self,
        window: WindowHandle32,
        menu: MenuHandle32,
        revision: u64,
        count: usize,
    ) -> Result<(), User32Error32> {
        let index = self.slot(window)?;
        let slot = &mut self.slots[index];
        if let Some(cache) = slot.menu_bar.as_mut().filter(|cache| cache.menu == menu) {
            if cache.measurements.len() < count {
                cache
                    .measurements
                    .try_reserve_exact(count - cache.measurements.len())
                    .map_err(|_| User32Error32::Allocation)?;
            }
            cache.measurements.resize(count, None);
            if cache.revision != revision {
                cache.revision = revision;
                cache.measurements.fill(None);
                cache.layout = None;
            }
            return Ok(());
        }
        let mut measurements = Vec::new();
        measurements
            .try_reserve_exact(count)
            .map_err(|_| User32Error32::Allocation)?;
        measurements.resize(count, None);
        slot.menu_bar = Some(MenuBarCache32 {
            menu,
            revision,
            measurements,
            layout: None,
            origin: Point32::default(),
            gutter: 0,
            padding: 0,
        });
        Ok(())
    }

    pub(crate) fn clear_menu_bar_cache(
        &mut self,
        window: WindowHandle32,
    ) -> Result<(), User32Error32> {
        let index = self.slot(window)?;
        self.slots[index].menu_bar = None;
        Ok(())
    }
}
