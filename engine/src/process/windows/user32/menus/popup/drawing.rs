use super::super::visual::{OwnerSize32, gdi_error};
use super::types::Stage32;
use crate::{
    memory::{AddressSpace, GuestAddress},
    process::windows::{
        gdi::GdiRuntime32,
        user32::{User32Context32, memory},
    },
    windows::user32::User32Error32 as E,
};

impl User32Context32 {
    pub(super) fn prepare_menu_measure(
        &mut self,
        token: u64,
        depth: usize,
        position: usize,
        ram: &mut AddressSpace,
        gdi: &GdiRuntime32,
    ) -> Result<(), E> {
        let index = self.popup_index(token)?;
        let plan = &self.menu_popups.plans[index];
        let item = self
            .menus
            .item(plan.windows[depth].menu, position as u32, true)?;
        // 32-bit MEASUREITEMSTRUCT, with only width and height supplied by the owner.
        // https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-measureitemstruct
        let font = gdi.system_font().map_err(gdi_error)?;
        let units = gdi.dialog_base_units(&font).map_err(gdi_error)?;
        let height = units.y;
        let padding = units.x.checked_mul(2).ok_or(E::Capacity)?;
        let words = [1u32, 0, item.id, 0, height as u32, item.data];
        let mut bytes = [0; 24];
        for (output, word) in bytes.chunks_exact_mut(4).zip(words) {
            output.copy_from_slice(&word.to_le_bytes());
        }
        ram.write(
            GuestAddress(self.popup_scratch(token)?.address.0 + 64),
            &bytes,
        )?;
        self.menu_popups.plans[index].measure_revision = self.menus.revision();
        self.menu_popups.plans[index].measure_padding = padding;
        self.menu_popups.plans[index].stage = Stage32::MeasureCall(depth, position);
        Ok(())
    }

    pub(super) fn accept_menu_measure(
        &mut self,
        token: u64,
        depth: usize,
        position: usize,
        ram: &AddressSpace,
    ) -> Result<(), E> {
        let index = self.popup_index(token)?;
        if self.menu_popups.plans[index].measure_revision != self.menus.revision() {
            self.menu_popups.plans[index].stage = Stage32::Layout(depth);
            return Ok(());
        }
        let words = memory::words::<6>(ram, self.popup_scratch(token)?.address.0 + 64)?;
        let width = i32::try_from(words[3]).map_err(|_| E::Win32(87))?;
        let height = i32::try_from(words[4]).map_err(|_| E::Win32(87))?;
        self.menu_popups.plans[index].windows[depth].measurements[position] = Some(OwnerSize32 {
            revision: self.menus.revision(),
            width: width
                .checked_add(self.menu_popups.plans[index].measure_padding)
                .ok_or(E::Capacity)?,
            height,
        });
        self.menu_popups.plans[index].stage = Stage32::Layout(depth);
        Ok(())
    }

    pub(super) fn prepare_menu_draw(
        &mut self,
        token: u64,
        depth: usize,
        mut position: usize,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
    ) -> Result<(), E> {
        let index = self.popup_index(token)?;
        let plan = &self.menu_popups.plans[index];
        let popup = &plan.windows[depth];
        let count = self.menus.count(popup.menu)?;
        while position < count
            && (self.menus.item(popup.menu, position as u32, true)?.kind & 0x100 == 0
                || plan
                    .draw_only
                    .is_some_and(|positions| !positions.contains(&Some(position))))
        {
            position += 1;
        }
        if position == count {
            let plan = &mut self.menu_popups.plans[index];
            if plan.exit_draw {
                plan.exit_draw = false;
                self.popup_after_drawing_exit(token)?;
            } else if plan.paint_notify {
                plan.paint_notify = false;
                plan.stage = Stage32::Select;
            } else {
                plan.stage = Stage32::Wait;
            }
            return Ok(());
        }
        let item = self.menus.item(popup.menu, position as u32, true)?;
        let mut rect = popup
            .visual
            .as_ref()
            .ok_or(E::InvalidRequest)?
            .layout
            .screen_item(position, Default::default())?;
        rect.left -= 1;
        rect.right -= 1;
        rect.top -= 1;
        rect.bottom -= 1;
        let window = popup.handle.ok_or(E::InvalidRequest)?;
        let projection = self.gdi_window(window.get(), false)?;
        let output = GuestAddress(self.popup_scratch(token)?.address.0 + 64);
        ram.ensure_transaction_versions(1)?;
        ram.check_span(output, 48, crate::memory::Access::Write)?;
        let dc = gdi
            .acquire_dc(projection, plan.call.thread_id)
            .map_err(gdi_error)?;
        let selected = !plan.exit_draw
            && plan.tracker.level(depth).and_then(|level| level.selected) == Some(position);
        let state = u32::from(selected)
            | if item.state & 1 != 0 { 2 } else { 0 }
            | if item.state & 2 != 0 { 4 } else { 0 }
            | if item.state & 8 != 0 { 8 } else { 0 }
            | if item.state & 0x1000 != 0 { 0x20 } else { 0 }
            | if plan.hide_accel { 0x100 } else { 0 };
        // DRAWITEMSTRUCT owns a real canonical DC; no staged canvas spans this callback.
        // https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-drawitemstruct
        let words = [
            1u32,
            0,
            item.id,
            plan.draw_action,
            state,
            popup.menu.get(),
            dc,
            rect.left as u32,
            rect.top as u32,
            rect.right as u32,
            rect.bottom as u32,
            item.data,
        ];
        let mut bytes = [0; 48];
        for (output, word) in bytes.chunks_exact_mut(4).zip(words) {
            output.copy_from_slice(&word.to_le_bytes());
        }
        let result = ram.write(output, &bytes);
        if let Err(error) = result {
            assert!(
                gdi.release_window_dc(window.get(), dc, plan.call.thread_id)
                    .map_err(gdi_error)?
            );
            return Err(error.into());
        }
        let plan = &mut self.menu_popups.plans[index];
        plan.draw_dc = Some((window, dc));
        plan.stage = Stage32::DrawCall(depth, position);
        Ok(())
    }

    pub(super) fn finish_menu_draw(
        &mut self,
        token: u64,
        depth: usize,
        position: usize,
        gdi: &mut GdiRuntime32,
    ) -> Result<(), E> {
        let index = self.popup_index(token)?;
        let plan = &self.menu_popups.plans[index];
        let (window, dc) = plan.draw_dc.ok_or(E::InvalidRequest)?;
        if !gdi
            .release_window_dc(window.get(), dc, plan.call.thread_id)
            .map_err(gdi_error)?
            && self.windows().get(window).is_ok()
        {
            return Err(E::Win32(6));
        }
        let plan = &mut self.menu_popups.plans[index];
        plan.draw_dc = None;
        plan.stage = Stage32::DrawPrepare(depth, position + 1);
        Ok(())
    }
}
