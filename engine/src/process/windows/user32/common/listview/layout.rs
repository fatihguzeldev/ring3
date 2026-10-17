use super::*;
#[derive(Clone, Copy)]
pub(super) struct Geometry32 {
    pub width: i32,
    pub height: i32,
    pub row: i32,
    pub header: i32,
    pub view: u32,
}
impl User32Context32 {
    pub(super) fn listview_geometry(
        &self,
        window: WindowHandle32,
        gdi: &GdiRuntime32,
    ) -> Result<Geometry32, HostError> {
        let extent = self
            .windows()
            .get(window)
            .map_err(E::from)
            .map_err(HostError::User32)?
            .description
            .extent;
        let style = self.state.record(window).map_err(HostError::User32)?.style;
        let row = gdi
            .font_metrics(self.listview_font(window)?)
            .map_err(HostError::Gdi)?
            .height
            .saturating_add(4)
            .max(1);
        let header = if style & 3 == 1 && style & 0x4000 == 0 {
            self.listview(window)
                .map_err(HostError::User32)?
                .header
                .and_then(|header| self.windows().get(header).ok())
                .map_or(row - 2, |record| record.description.extent.height as i32)
        } else {
            0
        };
        Ok(Geometry32 {
            width: extent.width as i32,
            height: extent.height as i32,
            row,
            header,
            view: style & 3,
        })
    }
    pub(super) fn listview_hit(
        &self,
        window: WindowHandle32,
        point: Point32,
        geometry: Geometry32,
    ) -> Result<(i32, i32, u32), E> {
        let list = self.listview(window)?;
        let outside = if point.x < 0 {
            0x40
        } else if point.x >= geometry.width {
            0x20
        } else {
            0
        } | if point.y < 0 {
            8
        } else if point.y >= geometry.height {
            0x10
        } else {
            0
        };
        if outside != 0 {
            return Ok((-1, 0, outside));
        }
        if point.y < geometry.header {
            return Ok((-1, 0, 1));
        }
        for (index, _) in list.items.iter().enumerate() {
            let rect = list.rect(
                index,
                geometry.view,
                geometry.width,
                geometry.height,
                geometry.row,
                geometry.header,
            );
            if point.x >= rect.left
                && point.x < rect.right
                && point.y >= rect.top
                && point.y < rect.bottom
            {
                let mut sub = 0;
                if geometry.view == 1 {
                    let header = self.listview_header(window)?;
                    let mut x = -list.horizontal;
                    for &column in &header.order {
                        let item = &header.items[column];
                        if point.x >= x && point.x < x + item.width {
                            sub = self.listview_column_subitem(window, column)? as i32;
                            break;
                        }
                        x += item.width;
                    }
                }
                return Ok((index as i32, sub, 4));
            }
        }
        Ok((-1, 0, 1))
    }
    pub(super) fn listview_geometry_message(
        &mut self,
        window: WindowHandle32,
        args: &[u32],
        ram: &mut AddressSpace,
        gdi: &GdiRuntime32,
    ) -> Result<u32, HostError> {
        let geometry = self.listview_geometry(window, gdi)?;
        let list = self.listview(window).map_err(HostError::User32)?;
        let index = args[2] as usize;
        match args[1] {
            0x100e | 0x1010 | 0x1038 => {
                if index >= list.items.len() {
                    return Ok(0);
                }
                let mut rect = list.rect(
                    index,
                    geometry.view,
                    geometry.width,
                    geometry.height,
                    geometry.row,
                    geometry.header,
                );
                if args[1] == 0x1010 {
                    let bytes = [rect.left, rect.top]
                        .into_iter()
                        .flat_map(i32::to_le_bytes)
                        .collect::<Vec<_>>();
                    ram.write(GuestAddress(args[3]), &bytes)
                        .map_err(HostError::Memory)?;
                } else {
                    let kind = memory::words::<1>(ram, args[3]).map_err(HostError::User32)?[0];
                    if args[1] == 0x1038 {
                        let sub = memory::words::<1>(
                            ram,
                            args[3]
                                .checked_add(4)
                                .ok_or(HostError::User32(E::Win32(87)))?,
                        )
                        .map_err(HostError::User32)?[0] as usize;
                        let header = self.listview_header(window).map_err(HostError::User32)?;
                        let Some(column) = header.order.iter().copied().find(|column| {
                            self.listview_column_subitem(window, *column).ok() == Some(sub)
                        }) else {
                            return Ok(0);
                        };
                        let col = header.rect(column, geometry.header).unwrap();
                        rect.left = col.left - list.horizontal;
                        rect.right = col.right - list.horizontal;
                    }
                    if kind == 1 {
                        rect.right = rect.left;
                    } else if kind > 3 {
                        return Ok(0);
                    }
                    memory::write_rect(ram, args[3], rect).map_err(HostError::User32)?;
                }
                Ok(1)
            }
            0x1012 => {
                let [x, y] = memory::words::<2>(ram, args[3]).map_err(HostError::User32)?;
                let (item, sub, flags) = self
                    .listview_hit(
                        window,
                        Point32 {
                            x: x as i32,
                            y: y as i32,
                        },
                        geometry,
                    )
                    .map_err(HostError::User32)?;
                let bytes = [flags, item as u32, sub as u32]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>();
                ram.write(
                    GuestAddress(
                        args[3]
                            .checked_add(8)
                            .ok_or(HostError::User32(E::Win32(87)))?,
                    ),
                    &bytes,
                )
                .map_err(HostError::Memory)?;
                Ok(item as u32)
            }
            0x1013 => {
                if index >= list.items.len() {
                    return Ok(0);
                }
                let rect = list.rect(
                    index,
                    geometry.view,
                    geometry.width,
                    geometry.height,
                    geometry.row,
                    geometry.header,
                );
                if rect.top < geometry.header || rect.bottom > geometry.height {
                    self.state.check_paint_change().map_err(HostError::User32)?;
                    let rows = ((geometry.height - geometry.header) / geometry.row).max(1) as usize;
                    let top = if rect.top < geometry.header {
                        index
                    } else {
                        index.saturating_sub(rows - 1)
                    };
                    self.listview_mut(window).map_err(HostError::User32)?.top = top;
                    self.listview_changed(window);
                }
                Ok(1)
            }
            0x1014 => {
                self.state.check_paint_change().map_err(HostError::User32)?;
                let list = self.listview_mut(window).map_err(HostError::User32)?;
                list.horizontal = list.horizontal.saturating_add(args[2] as i32).max(0);
                let delta = (args[3] as i32) / geometry.row;
                list.top = (list.top as i64 + i64::from(delta))
                    .clamp(0, list.items.len().saturating_sub(1) as i64)
                    as usize;
                self.listview_changed(window);
                Ok(1)
            }
            0x1015 => {
                if index >= list.items.len()
                    || args[3] as usize >= list.items.len()
                    || index > args[3] as usize
                {
                    return Ok(0);
                }
                self.state.check_paint_change().map_err(HostError::User32)?;
                self.listview_changed(window);
                Ok(1)
            }
            0x1027 => Ok(list.top as u32),
            0x1028 => Ok(((geometry.height - geometry.header) / geometry.row).max(0) as u32),
            _ => Err(HostError::InvalidRequest),
        }
    }
}
