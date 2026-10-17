use super::super::{
    layout::popup_origin,
    visual::{MenuPaint32, MenuVisual32, gdi_error},
};
use super::types::Stage32;
use crate::{
    memory::AddressSpace,
    process::windows::{
        gdi::GdiRuntime32,
        user32::{User32Context32, User32Outcome32},
    },
    windows::{
        gdi::Region32,
        user32::{Point32, User32Api32 as Api, User32Error32 as E},
    },
};

impl User32Context32 {
    pub(super) fn layout_popup(
        &mut self,
        token: u64,
        depth: usize,
        _ram: &AddressSpace,
        gdi: &mut GdiRuntime32,
    ) -> Result<(), E> {
        let index = self.popup_index(token)?;
        let menu = self.menu_popups.plans[index].windows[depth].menu;
        let count = self.menus.count(menu)?;
        let measurements = &mut self.menu_popups.plans[index].windows[depth].measurements;
        if measurements.len() < count {
            measurements
                .try_reserve(count - measurements.len())
                .map_err(|_| E::Allocation)?;
            measurements.resize(count, None);
        }
        for position in 0..count {
            if self.menus.item(menu, position as u32, true)?.kind & 0x100 != 0
                && self.menu_popups.plans[index].windows[depth].measurements[position]
                    .is_none_or(|size| size.revision != self.menus.revision())
            {
                self.menu_popups.plans[index].stage = Stage32::MeasurePrepare(depth, position);
                return Ok(());
            }
        }
        let plan = &self.menu_popups.plans[index];
        let owner = self.gdi_window(plan.owner.get(), true)?;
        let dc = gdi
            .acquire_dc(owner, plan.call.thread_id)
            .map_err(gdi_error)?;
        let measured = MenuVisual32::measure(
            gdi,
            dc,
            &self.menus,
            plan.windows[depth].menu,
            true,
            0,
            &plan.windows[depth].measurements,
        );
        assert!(
            gdi.release_window_dc(owner.window, dc, plan.call.thread_id)
                .map_err(gdi_error)?
        );
        let visual = measured?;
        let anchor = if depth == 0 {
            plan.anchor
        } else {
            let parent = &plan.windows[depth - 1];
            let rect = parent
                .visual
                .as_ref()
                .ok_or(E::InvalidRequest)?
                .layout
                .screen_item(plan.windows[depth].position, parent.origin)?;
            Point32 {
                x: if plan.flags & 0x8000 != 0 {
                    rect.left - visual.layout.extent.x
                } else {
                    rect.right
                },
                y: rect.top,
            }
        };
        let origin = popup_origin(
            anchor,
            visual.layout.extent,
            self.input.desktop,
            if depth == 0 { plan.flags } else { 0 },
            if depth == 0 { plan.exclude } else { None },
        )?;
        let stage = if let Some(window) = plan.windows[depth].handle {
            let current = self.state.record(window)?.outer;
            if current.left == origin.x
                && current.top == origin.y
                && current.right - current.left == visual.layout.extent.x
                && current.bottom - current.top == visual.layout.extent.y
            {
                Stage32::Paint(depth)
            } else {
                Stage32::Position(depth)
            }
        } else {
            Stage32::Create(depth)
        };
        let plan = &mut self.menu_popups.plans[index];
        if plan.windows[depth]
            .visual
            .as_ref()
            .is_none_or(|previous| previous.revision != visual.revision)
        {
            plan.draw_action = 1;
            plan.draw_only = None;
        }
        plan.windows[depth].visual = Some(visual);
        plan.windows[depth].origin = origin;
        plan.stage = stage;
        Ok(())
    }

    pub(super) fn paint_popup(
        &mut self,
        token: u64,
        depth: usize,
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
    ) -> Result<(), E> {
        let plan = &self.menu_popups.plans[self.popup_index(token)?];
        let popup = &plan.windows[depth];
        let window = popup.handle.ok_or(E::InvalidRequest)?;
        let projection = self.gdi_window(window.get(), true)?;
        let dc = gdi
            .acquire_dc(projection, plan.call.thread_id)
            .map_err(gdi_error)?;
        let result = (|| {
            let mut canvas = gdi.begin_canvas(ram, dc).map_err(gdi_error)?;
            popup.visual.as_ref().ok_or(E::InvalidRequest)?.draw(
                gdi,
                ram,
                &mut canvas,
                &self.menus,
                MenuPaint32 {
                    handle: popup.menu,
                    origin: Point32::default(),
                    selected: if plan.exit_draw {
                        None
                    } else {
                        plan.tracker.level(depth).and_then(|level| level.selected)
                    },
                    dirty: plan.draw_only,
                },
            )?;
            gdi.publish_canvas(ram, canvas).map_err(gdi_error)
        })();
        assert!(
            gdi.release_window_dc(projection.window, dc, plan.call.thread_id)
                .map_err(gdi_error)?
        );
        result?;
        self.state
            .set_update_region(window, Region32::default(), false)?;
        Ok(())
    }

    pub(in crate::process::windows::user32) fn popup_system_message(
        &mut self,
        api: Api,
        args: &[u32],
        ram: &mut AddressSpace,
        gdi: &mut GdiRuntime32,
    ) -> Option<Result<User32Outcome32, E>> {
        if !matches!(api, Api::DefWindowProcA | Api::DefWindowProcW) {
            return None;
        }
        let found = self.menu_popups.plans.iter().find_map(|plan| {
            plan.windows
                .iter()
                .position(|popup| popup.handle.is_some_and(|window| window.get() == args[0]))
                .map(|depth| (plan.token, depth))
        });
        let (token, depth) = found?;
        match args[1] {
            0xf => Some(
                self.paint_popup(token, depth, ram, gdi)
                    .map(|_| User32Outcome32::Return(0)),
            ),
            0x14 => Some(Ok(User32Outcome32::Return(1))),
            0x1f => {
                let index = self.popup_index(token).expect("located popup plan");
                self.menu_popups.plans[index].cancelled = true;
                Some(Ok(User32Outcome32::Return(0)))
            }
            _ => None,
        }
    }
}
