use super::super::visual::{MenuPaint32, MenuVisual32, MenuVisualRef32, gdi_error};
use super::types::*;
use crate::{
    memory::{Access, AddressSpace, GuestAddress},
    process::windows::{
        gdi::GdiRuntime32,
        user32::{User32Context32, memory},
    },
    windows::user32::{
        MenuMetrics32, MenuOwnerSize32, Point32, User32Error32 as E, menus::MenuHandle32,
    },
};

impl User32Context32 {
    pub(super) fn prepare_menu_bar_measure(
        &mut self,
        token: u64,
        mut position: usize,
        ram: &mut AddressSpace,
        gdi: &GdiRuntime32,
    ) -> Result<(), E> {
        let call = self.menu_bar_wait(token).ok_or(E::InvalidToken)?;
        let menu = MenuHandle32::try_from(call.menu)?;
        let count = self.menus.count(menu)?;
        self.state
            .prepare_menu_bar_cache(call.window, menu, self.menus.revision(), count)?;
        let cache = self
            .state
            .menu_bar_cache(call.window)?
            .ok_or(E::InvalidRequest)?;
        while position < count
            && (self.menus.item(menu, position as u32, true)?.kind & 0x100 == 0
                || cache.measurements[position]
                    .is_some_and(|size| size.revision == self.menus.revision()))
        {
            position += 1;
        }
        if position == count {
            return self.menu_bar_stage(token, Stage32::Layout);
        }
        let item = self.menus.item(menu, position as u32, true)?;
        let units = gdi
            .dialog_base_units(&gdi.system_font().map_err(gdi_error)?)
            .map_err(gdi_error)?;
        let mut bytes = [0; 24];
        for (output, value) in
            bytes
                .chunks_exact_mut(4)
                .zip([1, 0, item.id, 0, units.y as u32, item.data])
        {
            output.copy_from_slice(&value.to_le_bytes());
        }
        ram.write(GuestAddress(call.scratch.address.0 + 96), &bytes)?;
        let mut next = call;
        next.measure_revision = self.menus.revision();
        next.measure_padding = units.x.checked_mul(2).ok_or(E::Capacity)?;
        next.stage = Stage32::MeasureCall(position);
        self.publish_menu_bar(token, next);
        Ok(())
    }

    pub(super) fn accept_menu_bar_measure(
        &mut self,
        token: u64,
        position: usize,
        ram: &AddressSpace,
    ) -> Result<(), E> {
        let call = self.menu_bar_wait(token).ok_or(E::InvalidToken)?;
        if call.measure_revision != self.menus.revision() {
            return self.menu_bar_stage(token, Stage32::Measure(0));
        }
        let words = memory::words::<6>(ram, call.scratch.address.0 + 96)?;
        let width = i32::try_from(words[3])
            .map_err(|_| E::Win32(87))?
            .checked_add(call.measure_padding)
            .ok_or(E::Capacity)?;
        self.state.menu_bar_cache_mut(call.window)?.measurements[position] =
            Some(MenuOwnerSize32 {
                revision: self.menus.revision(),
                width,
                height: MenuMetrics32::menu_height() - 1,
            });
        self.menu_bar_stage(token, Stage32::Measure(position + 1))
    }

    pub(super) fn layout_menu_bar(&mut self, token: u64, gdi: &mut GdiRuntime32) -> Result<(), E> {
        let call = self.menu_bar_wait(token).ok_or(E::InvalidToken)?;
        let menu = MenuHandle32::try_from(call.menu)?;
        let count = self.menus.count(menu)?;
        self.state
            .prepare_menu_bar_cache(call.window, menu, self.menus.revision(), count)?;
        let cache = self
            .state
            .menu_bar_cache(call.window)?
            .ok_or(E::InvalidRequest)?;
        if (0..count).any(|position| {
            self.menus
                .item(menu, position as u32, true)
                .is_ok_and(|item| item.kind & 0x100 != 0 && cache.measurements[position].is_none())
        }) {
            return self.menu_bar_stage(token, Stage32::Measure(0));
        }
        let data = self.state.record(call.window)?;
        let origin = if let Purpose32::NonClient { base, .. } = call.purpose {
            base
        } else {
            self.default_client_rect(call.window, data.outer)?
        };
        let width = (origin.right - origin.left).max(1);
        let window = self.gdi_window(call.window.get(), true)?;
        let dc = gdi
            .acquire_dc(window, call.call.thread_id)
            .map_err(gdi_error)?;
        let measured = MenuVisual32::measure(
            gdi,
            dc,
            &self.menus,
            menu,
            false,
            width,
            &cache.measurements,
        );
        assert!(
            gdi.release_window_dc(window.window, dc, call.call.thread_id)
                .map_err(gdi_error)?
        );
        let visual = measured?;
        let cache = self.state.menu_bar_cache_mut(call.window)?;
        cache.layout = Some(visual.layout);
        cache.origin = Point32 {
            x: origin.left,
            y: origin.top,
        };
        cache.gutter = visual.gutter;
        cache.padding = visual.padding;
        self.menu_bar_stage(
            token,
            if matches!(call.purpose, Purpose32::NonClient { .. }) {
                Stage32::Finish
            } else {
                Stage32::Paint
            },
        )
    }

    pub(super) fn paint_menu_bar(
        &mut self,
        token: u64,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
    ) -> Result<(), E> {
        let call = self.menu_bar_wait(token).ok_or(E::InvalidToken)?;
        let data = self.state.record(call.window)?;
        let cache = self
            .state
            .menu_bar_cache(call.window)?
            .ok_or(E::InvalidRequest)?;
        let layout = cache.layout.as_ref().ok_or(E::InvalidRequest)?;
        let window = self.gdi_window(call.window.get(), true)?;
        let dc = gdi
            .acquire_dc(window, call.call.thread_id)
            .map_err(gdi_error)?;
        let result = (|| {
            let mut canvas = gdi.begin_canvas(ram, dc).map_err(gdi_error)?;
            MenuVisualRef32 {
                layout,
                popup: false,
                gutter: cache.gutter,
                padding: cache.padding,
            }
            .draw(
                gdi,
                ram,
                &mut canvas,
                &self.menus,
                MenuPaint32 {
                    handle: cache.menu,
                    origin: Point32 {
                        x: cache.origin.x - data.outer.left,
                        y: cache.origin.y - data.outer.top,
                    },
                    selected: None,
                    dirty: None,
                },
            )?;
            gdi.publish_canvas(ram, canvas).map_err(gdi_error)
        })();
        assert!(
            gdi.release_window_dc(window.window, dc, call.call.thread_id)
                .map_err(gdi_error)?
        );
        result?;
        self.menu_bar_stage(token, Stage32::Draw(0))
    }

    pub(super) fn draw_menu_bar_item(
        &mut self,
        token: u64,
        mut position: usize,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
    ) -> Result<(), E> {
        let call = self.menu_bar_wait(token).ok_or(E::InvalidToken)?;
        let menu = MenuHandle32::try_from(call.menu)?;
        let count = self.menus.count(menu)?;
        while position < count && self.menus.item(menu, position as u32, true)?.kind & 0x100 == 0 {
            position += 1;
        }
        if position == count {
            return self.menu_bar_stage(token, Stage32::Finish);
        }
        let data = self.state.record(call.window)?;
        let cache = self
            .state
            .menu_bar_cache(call.window)?
            .ok_or(E::InvalidRequest)?;
        let rect = cache
            .layout
            .as_ref()
            .ok_or(E::InvalidRequest)?
            .screen_item(
                position,
                Point32 {
                    x: cache.origin.x - data.outer.left,
                    y: cache.origin.y - data.outer.top,
                },
            )?;
        let item = self.menus.item(menu, position as u32, true)?;
        let state = (item.state & 8)
            | if item.state & 1 != 0 { 2 } else { 0 }
            | if item.state & 2 != 0 { 4 } else { 0 }
            | if item.state & 0x1000 != 0 { 0x20 } else { 0 }
            | if !self.input.last_ui_keyboard {
                0x100
            } else {
                0
            };
        let output = GuestAddress(call.scratch.address.0 + 64);
        ram.ensure_transaction_versions(1)?;
        ram.check_span(output, 48, Access::Write)?;
        let dc = gdi
            .acquire_dc(
                self.gdi_window(call.window.get(), true)?,
                call.call.thread_id,
            )
            .map_err(gdi_error)?;
        let mut bytes = [0; 48];
        for (output, value) in bytes.chunks_exact_mut(4).zip([
            1,
            0,
            item.id,
            1,
            state,
            menu.get(),
            dc,
            rect.left as u32,
            rect.top as u32,
            rect.right as u32,
            rect.bottom as u32,
            item.data,
        ]) {
            output.copy_from_slice(&value.to_le_bytes());
        }
        ram.write(output, &bytes)
            .expect("preflighted owner-draw scratch write");
        let mut next = call;
        next.dc = Some(dc);
        next.stage = Stage32::DrawCall(position);
        self.publish_menu_bar(token, next);
        Ok(())
    }

    pub(super) fn finish_menu_bar_dc(
        &self,
        call: MenuBar32,
        gdi: &mut GdiRuntime32,
    ) -> Result<(), E> {
        if let Some(dc) = call.dc {
            if !gdi
                .release_window_dc(call.window.get(), dc, call.call.thread_id)
                .map_err(gdi_error)?
                && self.windows().get(call.window).is_ok()
            {
                return Err(E::Win32(6));
            }
        }
        Ok(())
    }
}
