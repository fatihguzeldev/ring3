use super::*;
impl User32Context32 {
    fn read_tree_item(
        &self,
        window: WindowHandle32,
        pointer: u32,
        wide: bool,
        ram: &AddressSpace,
        call: User32CallContext32,
        old: Option<u32>,
    ) -> Result<(Item32, u32), E> {
        let words = memory::words::<10>(ram, pointer)?;
        let mask = words[0];
        if mask & !0x7f != 0 {
            return Err(E::Win32(50));
        }
        if mask & 2 != 0 && (words[6] as i32) < 0
            || mask & 0x20 != 0 && (words[7] as i32) < 0
            || mask & 0x40 != 0 && (words[8] as i32) < 0
        {
            return Err(E::Win32(50));
        }
        if mask & 8 != 0 && words[3] & !0x6c != 0 {
            return Err(E::Win32(50));
        }
        let text = if mask & 1 != 0 {
            if words[4] == u32::MAX {
                return Err(E::Win32(50));
            }
            memory::string(ram, words[4], wide, MAX_TEXT, call.ansi)?
        } else {
            Vec::new()
        };
        self.tree(window)?.check_text(old, &text)?;
        let item = Item32 {
            id: words[1],
            parent: 0,
            revision: 1,
            text,
            state: if mask & 8 != 0 {
                words[2] & words[3]
            } else {
                0
            },
            image: if mask & 2 != 0 { words[6] as i32 } else { 0 },
            selected_image: if mask & 0x20 != 0 { words[7] as i32 } else { 0 },
            children: if mask & 0x40 != 0 { words[8] as i32 } else { 0 },
            data: if mask & 4 != 0 { words[9] } else { 0 },
        };
        Ok((item, mask))
    }
    pub(super) fn insert_tree_item(
        &mut self,
        window: WindowHandle32,
        pointer: u32,
        wide: bool,
        ram: &AddressSpace,
        call: User32CallContext32,
    ) -> Result<u32, E> {
        let [parent, after] = memory::words::<2>(ram, pointer)?;
        if after == 0xffff0003 {
            return Err(E::Win32(50));
        }
        let (item_pointer, _) = pointer
            .checked_add(8)
            .zip(pointer.checked_add(48))
            .ok_or(E::Win32(87))?;
        let (mut item, _) = self.read_tree_item(window, item_pointer, wide, ram, call, None)?;
        item.parent = if parent == ROOT { 0 } else { parent };
        let id = self.state.prepare_tree_item_handle()?;
        item.id = id;
        self.state.check_paint_change()?;
        let index = self
            .tree_mut(window)?
            .prepare_insert(item.parent, after, &item)?;
        let drop_target = item.state & 8 != 0;
        self.tree_mut(window)?.insert(index, item);
        if drop_target {
            self.tree_mut(window)?.drop_target = id;
        }
        self.state.commit_tree_item_handle(id);
        self.tree_changed(window);
        Ok(id)
    }
    pub(super) fn get_tree_item(
        &self,
        window: WindowHandle32,
        pointer: u32,
        wide: bool,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<u32, E> {
        let mut words = memory::words::<10>(ram, pointer)?;
        let mask = words[0];
        if mask & !0x7f != 0 {
            return Err(E::Win32(50));
        }
        let tree = self.tree(window)?;
        let Some(item) = tree.item(words[1]) else {
            return Ok(0);
        };
        if mask & 8 != 0 {
            words[2] = tree.state(item) & words[3];
        }
        if mask & 2 != 0 {
            words[6] = item.image as u32;
        }
        if mask & 0x20 != 0 {
            words[7] = item.selected_image as u32;
        }
        if mask & 0x40 != 0 {
            words[8] = if tree.first_child(item.id) != 0 {
                1
            } else {
                item.children as u32
            };
        }
        if mask & 4 != 0 {
            words[9] = item.data;
        }
        let text = if mask & 1 != 0 && words[5] != 0 {
            memory::encoded_text(words[5] as usize, &item.text, wide, call)?.0
        } else {
            Vec::new()
        };
        let mut bytes = [0u8; 40];
        for (out, value) in bytes.chunks_exact_mut(4).zip(words) {
            out.copy_from_slice(&value.to_le_bytes());
        }
        ram.write_spans(&[
            (GuestAddress(pointer), &bytes),
            (GuestAddress(words[4]), &text),
        ])?;
        Ok(1)
    }
    pub(super) fn set_tree_item(
        &mut self,
        window: WindowHandle32,
        pointer: u32,
        wide: bool,
        ram: &AddressSpace,
        call: User32CallContext32,
    ) -> Result<u32, E> {
        let words = memory::words::<10>(ram, pointer)?;
        let id = words[1];
        let Some(old) = self.tree(window)?.item(id) else {
            return Ok(0);
        };
        let revision = old.revision.checked_add(1).ok_or(E::Capacity)?;
        let (mut item, mask) = self.read_tree_item(window, pointer, wide, ram, call, Some(id))?;
        if mask & 8 != 0 && words[3] & 0x20 != 0 && (item.state ^ old.state) & 0x20 != 0 {
            return Err(E::Win32(50));
        }
        self.state.check_paint_change()?;
        if mask & 8 != 0 && words[3] & 8 != 0 {
            let tree = self.tree_mut(window)?;
            if item.state & 8 != 0 {
                tree.drop_target = id;
            } else if tree.drop_target == id {
                tree.drop_target = 0;
            }
        }
        let node = self.tree_mut(window)?.item_mut(id).expect("same tree item");
        if mask & 1 != 0 {
            core::mem::swap(&mut node.text, &mut item.text);
        }
        if mask & 8 != 0 {
            node.state = (node.state & !words[3]) | item.state;
        }
        if mask & 2 != 0 {
            node.image = item.image;
        }
        if mask & 0x20 != 0 {
            node.selected_image = item.selected_image;
        }
        if mask & 0x40 != 0 {
            node.children = item.children;
        }
        if mask & 4 != 0 {
            node.data = item.data;
        }
        node.revision = revision;
        self.tree_changed(window);
        Ok(1)
    }
}
