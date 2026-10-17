use super::super::{User32CallContext32, User32Context32, memory};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::user32::{
        User32Api32 as Api, User32Error32 as E,
        menus::{MenuHandle32, MenuItem32},
    },
};

struct ItemInfo32 {
    words: [u32; 12],
    size: usize,
}

impl ItemInfo32 {
    fn read(ram: &AddressSpace, pointer: u32) -> Result<Self, E> {
        let size = memory::words::<1>(ram, pointer)?[0] as usize;
        if !matches!(size, 44 | 48) {
            return Err(E::Win32(87));
        }
        let mut bytes = [0; 48];
        ram.read(GuestAddress(pointer), &mut bytes[..size])?;
        let words = core::array::from_fn(|index| {
            u32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
        });
        let mask = words[1];
        if mask & !0x1ff != 0
            || mask & 0x10 != 0 && mask & 0x1c0 != 0
            || size == 44 && mask & 0x80 != 0
        {
            return Err(E::Win32(87));
        }
        Ok(Self { words, size })
    }

    fn bytes(&self) -> [u8; 48] {
        let mut bytes = [0; 48];
        for (index, word) in self.words.iter().enumerate() {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        bytes
    }
}

impl User32Context32 {
    pub(super) fn menu_item_information(
        &mut self,
        api: Api,
        args: &[u32],
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<u32, E> {
        let handle = MenuHandle32::try_from(args[0])?;
        let mut info = ItemInfo32::read(ram, args[3])?;
        let mask = info.words[1];
        let position = args[2] != 0;
        if matches!(api, Api::GetMenuItemInfoA | Api::GetMenuItemInfoW) {
            let item = self.menus.item(handle, args[1], position)?;
            if mask & 1 != 0 {
                info.words[3] = item.state;
            }
            if mask & 2 != 0 {
                info.words[4] = item.id;
            }
            if mask & 4 != 0 {
                info.words[5] = item.submenu.map_or(0, MenuHandle32::get);
            }
            if mask & 8 != 0 {
                info.words[6] = item.checked_bitmap;
                info.words[7] = item.unchecked_bitmap;
            }
            if mask & 0x20 != 0 {
                info.words[8] = item.data;
            }
            if mask & 0x80 != 0 {
                info.words[11] = item.bitmap;
            }
            if mask & 0x110 != 0 {
                info.words[2] = item.kind;
            }
            let mut text = Vec::new();
            let mut output = 0;
            if mask & 0x50 != 0 {
                if mask & 0x10 != 0 && item.kind & (4 | 0x100 | 0x800) != 0 {
                    info.words[9] = if item.kind & 4 != 0 { item.bitmap } else { 0 };
                    info.words[10] = 0;
                } else if info.words[9] == 0 || info.words[10] == 0 {
                    info.words[10] = if api.wide() {
                        item.text.len() as u32
                    } else {
                        call.ansi
                            .encode(&item.text, false)
                            .map_err(|_| E::Win32(1113))?
                            .0
                            .len() as u32
                    };
                } else {
                    output = info.words[9];
                    let (bytes, count) = memory::encoded_text(
                        info.words[10] as usize,
                        &item.text,
                        api.wide(),
                        call,
                    )?;
                    text = bytes;
                    info.words[10] = count;
                }
            }
            let bytes = info.bytes();
            if text.is_empty() {
                ram.write(GuestAddress(args[3]), &bytes[..info.size])?;
            } else {
                ram.write_spans(&[
                    (GuestAddress(args[3]), &bytes[..info.size]),
                    (GuestAddress(output), &text),
                ])?;
            }
            return Ok(1);
        }
        let inserting = matches!(api, Api::InsertMenuItemA | Api::InsertMenuItemW);
        let mut item = if inserting {
            MenuItem32::command(0, Vec::new())
        } else {
            let old = self.menus.item(handle, args[1], position)?;
            let mut text = Vec::new();
            text.try_reserve_exact(old.text.len())
                .map_err(|_| E::Allocation)?;
            text.extend_from_slice(&old.text);
            MenuItem32 {
                id: old.id,
                kind: old.kind,
                state: old.state,
                submenu: old.submenu,
                data: old.data,
                bitmap: old.bitmap,
                checked_bitmap: old.checked_bitmap,
                unchecked_bitmap: old.unchecked_bitmap,
                text,
            }
        };
        if mask & 1 != 0 {
            item.state = info.words[3];
        }
        if mask & 2 != 0 {
            item.id = info.words[4];
        }
        if mask & 4 != 0 {
            item.submenu = if info.words[5] == 0 {
                None
            } else {
                Some(MenuHandle32::try_from(info.words[5])?)
            };
        }
        if mask & 8 != 0 {
            item.checked_bitmap = info.words[6];
            item.unchecked_bitmap = info.words[7];
        }
        if mask & 0x110 != 0 {
            item.kind = info.words[2];
        }
        if mask & 0x50 != 0 {
            if mask & 0x10 != 0 && item.kind & (4 | 0x100 | 0x800) != 0 {
                item.text.clear();
                if item.kind & 4 != 0 {
                    item.bitmap = info.words[9];
                }
                if item.kind & 0x100 != 0 {
                    item.data = info.words[9];
                }
            } else {
                item.text = if info.words[9] == 0 {
                    Vec::new()
                } else {
                    memory::string(ram, info.words[9], api.wide(), 32767, call.ansi)?
                };
            }
        }
        if mask & 0x20 != 0 {
            item.data = info.words[8];
        }
        if mask & 0x80 != 0 {
            item.bitmap = info.words[11];
        }
        if inserting {
            let (handle, offset) = match self.menus.location(handle, args[1], position) {
                Ok(location) => location,
                Err(E::Win32(1456)) => (handle, self.menus.count(handle)?),
                Err(error) => return Err(error),
            };
            self.menus.insert(handle, offset, item)?;
        } else {
            self.menus.replace(handle, args[1], position, item)?;
        }
        Ok(1)
    }
}
