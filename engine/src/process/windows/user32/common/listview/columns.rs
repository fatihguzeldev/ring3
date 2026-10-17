use super::*;
use crate::windows::comctl32::header::{Header32, HeaderItem32};
impl User32Context32 {
    pub(super) fn listview_header(&self, window: WindowHandle32) -> Result<&Header32, E> {
        let header = self.listview(window)?.header.ok_or(E::InvalidRequest)?;
        match self.state.common_state(header)? {
            CommonControlState32::Header(state) => Ok(state),
            _ => Err(E::InvalidRequest),
        }
    }
    pub(super) fn listview_header_mut(
        &mut self,
        window: WindowHandle32,
    ) -> Result<&mut Header32, E> {
        let header = self.listview(window)?.header.ok_or(E::InvalidRequest)?;
        match self.state.common_state_mut(header)? {
            CommonControlState32::Header(state) => Ok(state),
            _ => Err(E::InvalidRequest),
        }
    }
    pub(super) fn listview_column_subitem(
        &self,
        window: WindowHandle32,
        index: usize,
    ) -> Result<usize, E> {
        let key = self
            .listview_header(window)?
            .items
            .get(index)
            .ok_or(E::Win32(87))?
            .key;
        Ok(self
            .listview(window)?
            .columns
            .iter()
            .find(|column| column.key == key)
            .map_or(index, |column| column.subitem))
    }
    fn listview_columns_changed(&mut self, window: WindowHandle32) {
        let header = self.listview(window).unwrap().header.unwrap();
        self.listview_changed(header);
        self.listview_changed(window);
    }
    pub(super) fn listview_column_message(
        &mut self,
        window: WindowHandle32,
        args: &[u32],
        ram: &mut AddressSpace,
        call: User32CallContext32,
        gdi: &mut GdiRuntime32,
    ) -> Result<User32Outcome32, HostError> {
        use crate::windows::comctl32::listview::Column32;
        let index = args[2] as usize;
        let value = match args[1] {
            0x101f => self
                .listview(window)
                .map_err(HostError::User32)?
                .header
                .map_or(0, WindowHandle32::get),
            0x101d => self
                .listview_header(window)
                .map_err(HostError::User32)?
                .items
                .get(index)
                .map_or(0, |item| item.width as u32),
            0x101e => {
                if index
                    >= self
                        .listview_header(window)
                        .map_err(HostError::User32)?
                        .items
                        .len()
                {
                    0
                } else {
                    let width = if args[3] as i32 >= 0 {
                        args[3] as i32
                    } else if args[3] as i32 >= -2 {
                        self.listview_auto_width(
                            window,
                            index,
                            args[3] as i32 == -2,
                            gdi,
                            call.thread_id,
                        )?
                    } else {
                        return Err(HostError::User32(E::Win32(87)));
                    };
                    let old = &self
                        .listview_header(window)
                        .map_err(HostError::User32)?
                        .items[index];
                    let item = HeaderItem32 {
                        key: old.key,
                        width,
                        text: Vec::new(),
                        format: old.format,
                        data: old.data,
                    };
                    self.listview_header(window)
                        .map_err(HostError::User32)?
                        .check_item(Some(index), &item)
                        .map_err(HostError::User32)?;
                    self.state.check_paint_change().map_err(HostError::User32)?;
                    self.listview_header_mut(window)
                        .map_err(HostError::User32)?
                        .items[index]
                        .width = width;
                    self.listview_columns_changed(window);
                    1
                }
            }
            0x101c => {
                let header = self.listview_header(window).map_err(HostError::User32)?;
                if index >= header.items.len() {
                    0
                } else {
                    let key = header.items[index].key;
                    self.state.check_paint_change().map_err(HostError::User32)?;
                    self.listview_header_mut(window)
                        .map_err(HostError::User32)?
                        .delete(index);
                    self.listview_mut(window)
                        .map_err(HostError::User32)?
                        .columns
                        .retain(|column| column.key != key);
                    self.listview_columns_changed(window);
                    1
                }
            }
            0x103b | 0x103a => {
                let header = self.listview_header(window).map_err(HostError::User32)?;
                if index != header.items.len() {
                    return Ok(User32Outcome32::Return(0));
                }
                if args[1] == 0x103b {
                    let bytes = header
                        .order
                        .iter()
                        .flat_map(|index| (*index as u32).to_le_bytes())
                        .collect::<Vec<_>>();
                    if !bytes.is_empty() {
                        ram.write(GuestAddress(args[3]), &bytes)
                            .map_err(HostError::Memory)?;
                    }
                } else {
                    let mut order = Vec::new();
                    order
                        .try_reserve_exact(index)
                        .map_err(|_| HostError::User32(E::Allocation))?;
                    for at in 0..index {
                        order.push(
                            memory::words::<1>(
                                ram,
                                args[3]
                                    .checked_add(at as u32 * 4)
                                    .ok_or(HostError::User32(E::Win32(87)))?,
                            )
                            .map_err(HostError::User32)?[0] as usize,
                        )
                    }
                    if header.check_order(&order).is_err() {
                        return Ok(User32Outcome32::Return(0));
                    }
                    self.state.check_paint_change().map_err(HostError::User32)?;
                    self.listview_header_mut(window)
                        .map_err(HostError::User32)?
                        .order = order;
                    self.listview_columns_changed(window);
                }
                1
            }
            0x1019 | 0x105f => {
                let mut words = memory::words::<6>(ram, args[3]).map_err(HostError::User32)?;
                let mask = words[0];
                if mask & !0x3f != 0 {
                    return Err(HostError::User32(E::Win32(50)));
                }
                let header = self.listview_header(window).map_err(HostError::User32)?;
                let Some(item) = header.items.get(index) else {
                    return Ok(User32Outcome32::Return(0));
                };
                if mask & 1 != 0 {
                    words[1] = item.format & 3
                }
                if mask & 2 != 0 {
                    words[2] = item.width as u32
                }
                if mask & 8 != 0 {
                    words[5] = self
                        .listview_column_subitem(window, index)
                        .map_err(HostError::User32)? as u32
                }
                let text = if mask & 4 != 0 && words[4] != 0 {
                    memory::encoded_text(words[4] as usize, &item.text, args[1] == 0x105f, call)
                        .map_err(HostError::User32)?
                        .0
                } else {
                    Vec::new()
                };
                let bytes = words
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>();
                let image = (-1i32).to_le_bytes();
                let order = (header.position(index).unwrap() as u32).to_le_bytes();
                let mut spans = vec![
                    (GuestAddress(args[3]), bytes.as_slice()),
                    (GuestAddress(words[3]), text.as_slice()),
                ];
                if mask & 16 != 0 {
                    spans.push((
                        GuestAddress(
                            args[3]
                                .checked_add(24)
                                .ok_or(HostError::User32(E::Win32(87)))?,
                        ),
                        image.as_slice(),
                    ))
                }
                if mask & 32 != 0 {
                    spans.push((
                        GuestAddress(
                            args[3]
                                .checked_add(28)
                                .ok_or(HostError::User32(E::Win32(87)))?,
                        ),
                        order.as_slice(),
                    ))
                }
                ram.write_spans(&spans).map_err(HostError::Memory)?;
                1
            }
            0x101a | 0x1060 | 0x101b | 0x1061 => {
                let insert = matches!(args[1], 0x101b | 0x1061);
                let wide = matches!(args[1], 0x1060 | 0x1061);
                let words = memory::words::<6>(ram, args[3]).map_err(HostError::User32)?;
                let mask = words[0];
                if mask & !0x3f != 0 || mask & 1 != 0 && words[1] & !3 != 0 {
                    return Err(HostError::User32(E::Win32(50)));
                }
                let header = self.listview_header(window).map_err(HostError::User32)?;
                if !insert && index >= header.items.len() {
                    return Ok(User32Outcome32::Return(0));
                }
                let old = (!insert).then(|| &header.items[index]);
                let text = if mask & 4 != 0 {
                    if words[3] == u32::MAX {
                        return Err(HostError::User32(E::Win32(50)));
                    }
                    if words[3] == 0 {
                        Vec::new()
                    } else {
                        memory::string(ram, words[3], wide, MAX_TEXT, call.ansi)
                            .map_err(HostError::User32)?
                    }
                } else {
                    let mut text = Vec::new();
                    if let Some(old) = old {
                        text.try_reserve_exact(old.text.len())
                            .map_err(|_| HostError::User32(E::Allocation))?;
                        text.extend_from_slice(&old.text)
                    }
                    text
                };
                let sub = if mask & 8 != 0 {
                    words[5] as usize
                } else if insert {
                    index
                } else {
                    self.listview_column_subitem(window, index)
                        .map_err(HostError::User32)?
                };
                if sub >= MAX_SUBITEMS {
                    return Err(HostError::User32(E::Win32(87)));
                }
                if mask & 16 != 0
                    && memory::words::<1>(
                        ram,
                        args[3]
                            .checked_add(24)
                            .ok_or(HostError::User32(E::Win32(87)))?,
                    )
                    .map_err(HostError::User32)?[0] as i32
                        >= 0
                {
                    return Err(HostError::User32(E::Win32(50)));
                }
                let order = if mask & 32 != 0 {
                    memory::words::<1>(
                        ram,
                        args[3]
                            .checked_add(28)
                            .ok_or(HostError::User32(E::Win32(87)))?,
                    )
                    .map_err(HostError::User32)?[0] as usize
                } else {
                    if insert {
                        index
                    } else {
                        header.position(index).unwrap()
                    }
                };
                let item = HeaderItem32 {
                    key: old.map_or(0, |item| item.key),
                    width: if mask & 2 != 0 {
                        words[2] as i32
                    } else {
                        old.map_or(0, |item| item.width)
                    },
                    text,
                    format: 0x4000
                        | if mask & 1 != 0 {
                            words[1] & 3
                        } else {
                            old.map_or(0, |item| item.format)
                        },
                    data: old.map_or(0, |item| item.data),
                };
                header
                    .check_item((!insert).then_some(index), &item)
                    .map_err(HostError::User32)?;
                let mut live_keys = Vec::new();
                live_keys
                    .try_reserve_exact(header.items.len())
                    .map_err(|_| HostError::User32(E::Allocation))?;
                live_keys.extend(header.items.iter().map(|item| item.key));
                self.listview_mut(window)
                    .map_err(HostError::User32)?
                    .columns
                    .try_reserve(1)
                    .map_err(|_| HostError::User32(E::Allocation))?;
                self.state.check_paint_change().map_err(HostError::User32)?;
                let at = if insert {
                    self.listview_mut(window)
                        .map_err(HostError::User32)?
                        .columns
                        .try_reserve(1)
                        .map_err(|_| HostError::User32(E::Allocation))?;
                    self.listview_header_mut(window)
                        .map_err(HostError::User32)?
                        .prepare_insert(&item)
                        .map_err(HostError::User32)?;
                    self.listview_header_mut(window)
                        .map_err(HostError::User32)?
                        .insert(index, order, item)
                } else {
                    let header = self
                        .listview_header_mut(window)
                        .map_err(HostError::User32)?;
                    header.items[index] = item;
                    header.move_item(index, order);
                    index
                };
                let key = self
                    .listview_header(window)
                    .map_err(HostError::User32)?
                    .items[at]
                    .key;
                let columns = &mut self
                    .listview_mut(window)
                    .map_err(HostError::User32)?
                    .columns;
                columns.retain(|column| column.key == key || live_keys.contains(&column.key));
                if let Some(column) = columns.iter_mut().find(|column| column.key == key) {
                    column.subitem = sub
                } else {
                    columns.push(Column32 { key, subitem: sub })
                }
                self.listview_columns_changed(window);
                if insert { at as u32 } else { 1 }
            }
            _ => return Err(HostError::InvalidRequest),
        };
        Ok(User32Outcome32::Return(value))
    }
    fn listview_auto_width(
        &self,
        window: WindowHandle32,
        index: usize,
        header_text: bool,
        gdi: &mut GdiRuntime32,
        thread: u32,
    ) -> Result<i32, HostError> {
        let projection = self
            .gdi_window(window.get(), false)
            .map_err(HostError::User32)?;
        let bounds = Rect32 {
            left: 0,
            top: 0,
            right: projection.width as i32,
            bottom: projection.height as i32,
        };
        let dc = gdi
            .begin_window_paint_dc(
                projection,
                Region32::rectangle(bounds),
                self.listview_font(window)?,
                gdi.system_color(5),
            )
            .map_err(HostError::Gdi)?;
        let result = (|| {
            let sub = self
                .listview_column_subitem(window, index)
                .map_err(HostError::User32)?;
            let mut width = 0;
            for item in &self.listview(window).map_err(HostError::User32)?.items {
                width = width.max(
                    (gdi.measure_text(dc, item.text(sub))
                        .map_err(HostError::Gdi)?
                        .advance
                        .ceil() as i32)
                        .saturating_add(8),
                );
            }
            if header_text {
                width = width.max(
                    (gdi.measure_text(
                        dc,
                        &self
                            .listview_header(window)
                            .map_err(HostError::User32)?
                            .items[index]
                            .text,
                    )
                    .map_err(HostError::Gdi)?
                    .advance
                    .ceil() as i32)
                        .saturating_add(8),
                )
            }
            Ok(width)
        })();
        assert!(
            gdi.release_window_dc(window.get(), dc, thread)
                .expect("column sizing owns its acquired dc")
        );
        result
    }
}
