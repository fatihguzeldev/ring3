use super::*;
impl User32Context32 {
    pub(super) fn get_listview_item(
        &self,
        window: WindowHandle32,
        args: &[u32],
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<u32, E> {
        let text_only = matches!(args[1], 0x102d | 0x1073);
        let wide = matches!(args[1], 0x104b | 0x1073);
        let mut words = if text_only {
            let mut out = [0; 9];
            out[..7].copy_from_slice(&memory::words::<7>(ram, args[3])?);
            out
        } else {
            memory::words::<9>(ram, args[3])?
        };
        let index = if text_only {
            args[2] as usize
        } else {
            words[1] as usize
        };
        let list = self.listview(window)?;
        let Some(item) = list.items.get(index) else {
            return Ok(0);
        };
        let sub = words[2] as usize;
        if sub >= MAX_SUBITEMS {
            return Ok(0);
        }
        let mask = if text_only { 1 } else { words[0] };
        if mask & !0x1f != 0 {
            return Err(E::Win32(50));
        }
        let (text, count) = if mask & 1 != 0 && words[6] != 0 {
            memory::encoded_text(words[6] as usize, item.text(sub), wide, call)?
        } else {
            (Vec::new(), 0)
        };
        if text_only {
            if !text.is_empty() {
                ram.write(GuestAddress(words[5]), &text)?;
            }
            return Ok(count);
        }
        if mask & 8 != 0 {
            words[3] = item.state & words[4]
        }
        if mask & 2 != 0 {
            words[7] = item.image as u32
        }
        if mask & 4 != 0 {
            words[8] = item.data
        }
        let bytes = words
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        let indent = item.indent.to_le_bytes();
        let mut spans = vec![
            (GuestAddress(args[3]), bytes.as_slice()),
            (GuestAddress(words[5]), text.as_slice()),
        ];
        if mask & 16 != 0 {
            spans.push((
                GuestAddress(args[3].checked_add(36).ok_or(E::Win32(87))?),
                &indent,
            ))
        }
        ram.write_spans(&spans)?;
        Ok(1)
    }
    pub(super) fn set_listview_item(
        &mut self,
        window: WindowHandle32,
        args: &[u32],
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        let insert = matches!(args[1], 0x1007 | 0x104d);
        let text_only = matches!(args[1], 0x102e | 0x1074);
        let state_only = args[1] == 0x102b;
        let wide = matches!(args[1], 0x104c | 0x104d | 0x1074);
        let words = if state_only {
            let mut out = [0; 9];
            out[..5].copy_from_slice(&memory::words::<5>(ram, args[3])?);
            out
        } else if text_only {
            let mut out = [0; 9];
            out[..6].copy_from_slice(&memory::words::<6>(ram, args[3])?);
            out
        } else {
            memory::words::<9>(ram, args[3])?
        };
        let mask = if state_only {
            8
        } else if text_only {
            1
        } else {
            words[0]
        };
        if mask & !0x1f != 0 {
            return Err(E::Win32(50));
        }
        let index = if state_only || text_only {
            args[2] as usize
        } else {
            words[1] as usize
        };
        let sub = if state_only { 0 } else { words[2] as usize };
        if sub >= MAX_SUBITEMS || insert && sub != 0 {
            return Ok(if insert { u32::MAX } else { 0 }).map(User32Outcome32::Return);
        }
        if mask & 8 != 0 && words[3] & words[4] & !15 != 0 {
            return Err(E::Win32(50));
        }
        let text = if mask & 1 != 0 {
            if words[5] == u32::MAX {
                return Err(E::Win32(50));
            }
            Some(if words[5] == 0 {
                Vec::new()
            } else {
                memory::string(ram, words[5], wide, MAX_TEXT, call.ansi)?
            })
        } else {
            None
        };
        let indent = if mask & 16 != 0 {
            memory::words::<1>(ram, args[3].checked_add(36).ok_or(E::Win32(87))?)?[0] as i32
        } else {
            0
        };
        if indent < 0 {
            return Err(E::Win32(87));
        }
        if insert {
            let mut cells = Vec::new();
            if let Some(text) = text {
                cells.try_reserve_exact(1).map_err(|_| E::Allocation)?;
                cells.push(Cell32 { subitem: 0, text })
            }
            let item = Item32 {
                key: 0,
                revision: 0,
                cells,
                state: if mask & 8 != 0 {
                    words[3] & words[4]
                } else {
                    0
                },
                image: if mask & 2 != 0 { words[7] as i32 } else { -1 },
                data: if mask & 4 != 0 { words[8] } else { 0 },
                indent,
                position: None,
            };
            let single = self.state.record(window)?.style & 4 != 0;
            if item.state & 1 != 0 || single && item.state & 2 != 0 {
                return self.begin_listview_insertion(window, index, item, ram, call);
            }
            self.listview_mut(window)?.prepare_insert(&item)?;
            self.state.check_paint_change()?;
            let index = index.min(self.listview(window)?.items.len());
            let note = self.prepare_listview_notice(
                window,
                -102,
                index as i32,
                0,
                item.state,
                0,
                mask,
                item.data,
                Point32::default(),
                index as u32,
                ram,
                call,
            )?;
            self.listview_mut(window)?.insert(index, item);
            self.listview_changed(window);
            return self.publish_listview_call(note);
        }
        let list = self.listview(window)?;
        if index >= list.items.len() && !(state_only && args[2] == u32::MAX) {
            return Ok(User32Outcome32::Return(0));
        }
        let mut patches = Vec::new();
        patches
            .try_reserve_exact(if state_only && args[2] == u32::MAX {
                list.items.len()
            } else {
                1
            })
            .map_err(|_| E::Allocation)?;
        let targets = if state_only && args[2] == u32::MAX {
            0..list.items.len()
        } else {
            index..index + 1
        };
        let mut text = text;
        for index in targets {
            let item = &list.items[index];
            patches.push(Patch32 {
                key: item.key,
                revision: item.revision,
                subitem: sub,
                mask,
                state: words[3],
                state_mask: words[4],
                image: words[7] as i32,
                data: words[8],
                indent,
                text: text.take(),
            });
        }
        if let Some(focus) = patches
            .iter()
            .rev()
            .find(|patch| patch.mask & 8 != 0 && patch.state_mask & 1 != 0 && patch.state & 1 != 0)
            .map(|patch| patch.key)
        {
            for patch in &mut patches {
                if patch.key != focus && patch.mask & 8 != 0 && patch.state_mask & 1 != 0 {
                    patch.state &= !1;
                }
            }
            for item in &list.items {
                if item.key != focus
                    && item.state & 1 != 0
                    && !patches.iter().any(|patch| patch.key == item.key)
                {
                    patches.try_reserve(1).map_err(|_| E::Allocation)?;
                    patches.insert(
                        0,
                        Patch32 {
                            key: item.key,
                            revision: item.revision,
                            subitem: 0,
                            mask: 8,
                            state: 0,
                            state_mask: 1,
                            image: 0,
                            data: 0,
                            indent: 0,
                            text: None,
                        },
                    );
                }
            }
        }
        self.begin_listview_changes(window, patches, ram, call, false, 1, None)
    }
    pub(super) fn delete_listview_items(
        &mut self,
        window: WindowHandle32,
        index: u32,
        all: bool,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        let list = self.listview(window)?;
        if !all && index as usize >= list.items.len() {
            return Ok(User32Outcome32::Return(0));
        }
        if all {
            return self.begin_listview_delete_all(window, ram, call);
        }
        let item = &list.items[index as usize];
        let note = self.prepare_listview_notice(
            window,
            -103,
            index as i32,
            0,
            0,
            item.state,
            0,
            item.data,
            Point32::default(),
            1,
            ram,
            call,
        )?;
        self.state.check_paint_change()?;
        self.listview_mut(window)?.delete(index as usize);
        self.listview_changed(window);
        self.publish_listview_call(note)
    }
    pub(super) fn find_listview_item(
        &self,
        window: WindowHandle32,
        args: &[u32],
        ram: &AddressSpace,
        call: User32CallContext32,
    ) -> Result<u32, E> {
        let [flags, text, data] = memory::words::<3>(ram, args[3])?;
        if flags & !0x2f != 0 {
            return Err(E::Win32(50));
        }
        let name = if flags & 2 != 0 {
            memory::string(ram, text, args[1] == 0x1053, MAX_TEXT, call.ansi)?
        } else {
            Vec::new()
        };
        let key = crate::windows::text::ordinal_key(&name);
        let list = self.listview(window)?;
        let start = (args[2] as i32).saturating_add(1).max(0) as usize;
        for index in (start..list.items.len()).chain(if flags & 0x20 != 0 {
            0..start.min(list.items.len())
        } else {
            0..0
        }) {
            let item = &list.items[index];
            let yes = if flags & 1 != 0 {
                item.data == data
            } else if flags & 2 != 0 {
                let candidate = crate::windows::text::ordinal_key(item.text(0));
                if flags & 8 != 0 {
                    candidate.starts_with(&key)
                } else if flags & 4 != 0 {
                    candidate.windows(key.len().max(1)).any(|part| part == key)
                } else {
                    candidate == key
                }
            } else {
                false
            };
            if yes {
                return Ok(index as u32);
            }
        }
        Ok(u32::MAX)
    }
}
