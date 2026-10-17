use super::*;
const MASK: u32 = 0x8f;
fn word(ram: &AddressSpace, pointer: u32, offset: u32) -> Result<u32, E> {
    memory::words::<1>(ram, pointer.checked_add(offset).ok_or(E::Win32(87))?).map(|v| v[0])
}
impl User32Context32 {
    pub(in crate::process::windows::user32::common::header) fn read_header_item(
        &self,
        window: WindowHandle32,
        index: Option<usize>,
        pointer: u32,
        wide: bool,
        ram: &AddressSpace,
        call: User32CallContext32,
    ) -> Result<(HeaderItem32, usize, u32), E> {
        let mask = word(ram, pointer, 0)?;
        if mask & !MASK != 0 {
            return Err(E::Win32(50));
        }
        let header = self.header(window)?;
        let old = index.and_then(|index| header.items.get(index));
        let mut text = Vec::new();
        if mask & 2 != 0 {
            let source = word(ram, pointer, 8)?;
            if source == u32::MAX {
                return Err(E::Win32(50));
            }
            if source != 0 {
                text = memory::string(ram, source, wide, MAX_TEXT, call.ansi)?;
            }
        } else if let Some(old) = old {
            text.try_reserve_exact(old.text.len())
                .map_err(|_| E::Allocation)?;
            text.extend_from_slice(&old.text);
        }
        let item = HeaderItem32 {
            key: old.map_or(0, |item| item.key),
            width: if mask & 1 != 0 {
                word(ram, pointer, 4)? as i32
            } else {
                old.map_or(0, |item| item.width)
            },
            format: if mask & 4 != 0 {
                word(ram, pointer, 20)?
            } else {
                old.map_or(if mask & 2 != 0 { 0x4000 } else { 0 }, |item| item.format)
            },
            data: if mask & 8 != 0 {
                word(ram, pointer, 24)?
            } else {
                old.map_or(0, |item| item.data)
            },
            text,
        };
        let position = if mask & 0x80 != 0 {
            word(ram, pointer, 32)? as usize
        } else {
            index
                .and_then(|index| header.position(index))
                .unwrap_or(header.items.len())
        };
        let count = header.items.len() + usize::from(index.is_none());
        if position >= count && mask & 0x80 != 0 {
            return Err(E::Win32(87));
        }
        header.check_item(index, &item)?;
        Ok((item, position, mask))
    }
    pub(in crate::process::windows::user32::common::header) fn get_header_item(
        &self,
        window: WindowHandle32,
        index: usize,
        pointer: u32,
        wide: bool,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<u32, E> {
        let Some(item) = self.header(window)?.items.get(index) else {
            return Ok(0);
        };
        let mask = word(ram, pointer, 0)?;
        if mask & !0xbf != 0 {
            return Err(E::Win32(50));
        }
        let mut writes = Vec::<(GuestAddress, Vec<u8>)>::new();
        writes.try_reserve_exact(8).map_err(|_| E::Allocation)?;
        for (flag, offset, value) in [
            (1, 4, item.width as u32),
            (4, 20, item.format),
            (8, 24, item.data),
            (0x10, 12, 0),
            (0x20, 28, u32::MAX),
            (
                0x80,
                32,
                self.header(window)?.position(index).unwrap() as u32,
            ),
        ] {
            if mask & flag != 0 {
                writes.push((
                    GuestAddress(pointer.checked_add(offset).ok_or(E::Win32(87))?),
                    value.to_le_bytes().to_vec(),
                ));
            }
        }
        if mask & 2 != 0 {
            let output = word(ram, pointer, 8)?;
            let count = word(ram, pointer, 16)? as usize;
            if count > MAX_TEXT + 1 {
                return Err(E::Win32(87));
            }
            let (text, _) = memory::encoded_text(count, &item.text, wide, call)?;
            if !text.is_empty() {
                writes.push((GuestAddress(output), text));
            }
        }
        let spans = writes
            .iter()
            .map(|(address, bytes)| (*address, bytes.as_slice()))
            .collect::<Vec<_>>();
        ram.write_spans(&spans)?;
        Ok(1)
    }
    pub(in crate::process::windows::user32::common::header) fn insert_header_item(
        &mut self,
        window: WindowHandle32,
        index: usize,
        pointer: u32,
        wide: bool,
        ram: &AddressSpace,
        call: User32CallContext32,
    ) -> Result<u32, E> {
        let (item, position, mask) =
            self.read_header_item(window, None, pointer, wide, ram, call)?;
        self.state.check_paint_change()?;
        self.header_mut(window)?.prepare_insert(&item)?;
        let index = index.min(self.header(window)?.items.len());
        let position = if mask & 0x80 != 0 { position } else { index };
        let result = self.header_mut(window)?.insert(index, position, item);
        self.header_changed(window);
        Ok(result as u32)
    }
}
