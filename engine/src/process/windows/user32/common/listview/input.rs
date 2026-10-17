use super::*;
impl User32Context32 {
    pub(super) fn listview_input(
        &mut self,
        window: WindowHandle32,
        args: &[u32],
        ram: &mut AddressSpace,
        gdi: &GdiRuntime32,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, HostError> {
        if !self
            .windows()
            .get(window)
            .map_err(E::from)
            .map_err(HostError::User32)?
            .description
            .enabled
        {
            return Ok(User32Outcome32::Return(0));
        }
        let geometry = self.listview_geometry(window, gdi)?;
        let point = Point32 {
            x: args[3] as u16 as i16 as i32,
            y: (args[3] >> 16) as u16 as i16 as i32,
        };
        if matches!(args[1], 0x202 | 0x205) {
            let (index, sub, _) = self
                .listview_hit(window, point, geometry)
                .map_err(HostError::User32)?;
            return self
                .begin_listview_click(
                    window,
                    if args[1] == 0x202 { -2 } else { -5 },
                    index,
                    sub,
                    point,
                    ram,
                    call,
                )
                .map_err(HostError::User32);
        }
        let list = self.listview(window).map_err(HostError::User32)?;
        let mouse = args[1] != 0x100;
        let index = if mouse {
            let (index, _, _) = self
                .listview_hit(window, point, geometry)
                .map_err(HostError::User32)?;
            if index < 0 {
                return Ok(User32Outcome32::Return(0));
            }
            index as usize
        } else {
            if list.items.is_empty() {
                return Ok(User32Outcome32::Return(0));
            }
            let old = list.focused().unwrap_or(0);
            match args[2] {
                0x26 => old.saturating_sub(1),
                0x28 => old.saturating_add(1).min(list.items.len() - 1),
                0x24 => 0,
                0x23 => list.items.len() - 1,
                0x21 => old.saturating_sub(
                    ((geometry.height - geometry.header) / geometry.row).max(1) as usize,
                ),
                0x22 => old
                    .saturating_add(
                        ((geometry.height - geometry.header) / geometry.row).max(1) as usize
                    )
                    .min(list.items.len() - 1),
                0x20 => old,
                _ => return Ok(User32Outcome32::Return(0)),
            }
        };
        let style = self.state.record(window).map_err(HostError::User32)?.style;
        let single = style & 4 != 0;
        let control = call.control && !single;
        let shift = call.shift && !single;
        let anchor = list
            .anchor
            .and_then(|key| list.index(key))
            .or_else(|| list.focused())
            .unwrap_or(index);
        let right_preserve = args[1] == 0x204 && list.items[index].state & 2 != 0;
        let mut patches = Vec::new();
        patches
            .try_reserve_exact(list.items.len())
            .map_err(|_| HostError::User32(E::Allocation))?;
        for (at, item) in list.items.iter().enumerate() {
            let selected = if right_preserve {
                item.state & 2 != 0
            } else if shift {
                if control {
                    item.state & 2 != 0 || (at >= anchor.min(index) && at <= anchor.max(index))
                } else {
                    at >= anchor.min(index) && at <= anchor.max(index)
                }
            } else if control {
                if at == index && (mouse || args[2] == 0x20) {
                    item.state & 2 == 0
                } else {
                    item.state & 2 != 0
                }
            } else {
                at == index
            };
            let next = (item.state & !3) | u32::from(at == index) | (u32::from(selected) << 1);
            if next != item.state {
                patches.push(Patch32 {
                    key: item.key,
                    revision: item.revision,
                    subitem: 0,
                    mask: 8,
                    state: next,
                    state_mask: 3,
                    image: 0,
                    data: 0,
                    indent: 0,
                    text: None,
                });
            }
        }
        let target = list.items[index].key;
        let click = (args[1] == 0x203).then_some((-3, index as i32, 0, point));
        let outcome = self
            .begin_listview_changes(window, patches, ram, call, mouse, 0, click)
            .map_err(HostError::User32)?;
        if !shift {
            self.listview_mut(window).map_err(HostError::User32)?.anchor = Some(target)
        }
        Ok(outcome)
    }
}
