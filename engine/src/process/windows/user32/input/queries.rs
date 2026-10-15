use super::super::{User32CallContext32, User32Context32, User32Outcome32, memory};
use super::context::User32Input32;
use crate::{
    input::keyboard::{self, KeyboardLayout32},
    memory::{AddressSpace, GuestAddress},
    windows::user32::{User32Api32, User32Error32},
};

impl User32Context32 {
    pub(in crate::process::windows::user32) fn input_query(
        &mut self,
        api: User32Api32,
        args: &[u32],
        memory: &mut AddressSpace,
        context: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        use User32Api32::*;
        let value = match api {
            GetAsyncKeyState => self.input.asynchronous_key(args[0] as usize),
            GetKeyState => self.input.queued.get(args[0] as usize).map_or(0, |&key| {
                (((u16::from(key & 0x80) << 8) | u16::from(key & 1)) as i16 as i32) as u32
            }),
            GetKeyboardState => {
                memory.write(GuestAddress(args[0]), &self.input.queued)?;
                1
            }
            SetKeyboardState => {
                let mut state = [0; 256];
                memory.read(GuestAddress(args[0]), &mut state)?;
                for value in &mut state {
                    *value &= 0x81;
                }
                self.input.queued = state;
                1
            }
            MapVirtualKeyA | MapVirtualKeyW | MapVirtualKeyExA | MapVirtualKeyExW => {
                let layout = if args.len() == 3 {
                    self.input_layout(args[2])?
                } else {
                    self.input.layout
                };
                if args[1] == 2 {
                    u8::try_from(args[0])
                        .ok()
                        .and_then(|key| {
                            keyboard::character(layout, key, false, true, false, false).map(
                                |character| {
                                    let character = if api.wide() {
                                        character as u32
                                    } else {
                                        u32::from(
                                            keyboard::encode_ansi(layout, character)
                                                .unwrap_or(b'?'),
                                        )
                                    };
                                    character
                                        | (u32::from(keyboard::dead_key(
                                            layout, key, false, false, false,
                                        )) << 31)
                                },
                            )
                        })
                        .unwrap_or(0)
                } else {
                    keyboard::map_virtual_key_for_layout(layout, args[0], args[1])
                }
            }
            GetKeyboardLayout => {
                if args[0] > 1 {
                    return Err(User32Error32::Win32(87));
                }
                self.input.layout.handle()
            }
            GetKeyboardLayoutList => {
                let layouts = [KeyboardLayout32::Us, KeyboardLayout32::TurkishQ];
                let count = self.input.loaded.iter().filter(|&&loaded| loaded).count();
                if args[0] == 0 {
                    count as u32
                } else {
                    if args[0] > i32::MAX as u32 {
                        return Err(User32Error32::Win32(87));
                    }
                    let mut bytes = [0; 8];
                    let mut written = 0;
                    for (layout, loaded) in layouts.into_iter().zip(self.input.loaded) {
                        if loaded && written < (args[0] as usize).min(2) {
                            bytes[written * 4..written * 4 + 4]
                                .copy_from_slice(&layout.handle().to_le_bytes());
                            written += 1;
                        }
                    }
                    memory.write(GuestAddress(args[1]), &bytes[..written * 4])?;
                    written as u32
                }
            }
            ActivateKeyboardLayout => {
                if args[1] & !0x118 != 0 {
                    return Err(User32Error32::Win32(50));
                }
                let next = if args[0] <= 1 {
                    if self.input.loaded.iter().all(|&value| value) {
                        match self.input.layout {
                            KeyboardLayout32::Us => KeyboardLayout32::TurkishQ,
                            KeyboardLayout32::TurkishQ => KeyboardLayout32::Us,
                        }
                    } else {
                        self.input.layout
                    }
                } else {
                    self.input_layout(args[0])?
                };
                if !self.input.loaded[layout_index(next)] {
                    return Err(User32Error32::Win32(6));
                }
                let previous = self.input.layout.handle();
                self.input.layout = next;
                if self.input.cursor.is_none() {
                    self.input.delivery_layout = next;
                }
                self.input.dead = None;
                previous
            }
            LoadKeyboardLayoutA | LoadKeyboardLayoutW => {
                if args[1] & !0x11b != 0 {
                    return Err(User32Error32::Win32(50));
                }
                let name = memory::string(memory, args[0], api.wide(), 8)?;
                if name.len() != 8 {
                    return Err(User32Error32::Win32(87));
                }
                let mut value = 0u32;
                for unit in name {
                    value = value
                        .checked_mul(16)
                        .and_then(|value| {
                            char::from_u32(u32::from(unit))
                                .and_then(|unit| unit.to_digit(16))
                                .map(|digit| value + digit)
                        })
                        .ok_or(User32Error32::Win32(87))?;
                }
                let layout =
                    KeyboardLayout32::from_handle(value).ok_or(User32Error32::Win32(50))?;
                self.input.loaded[layout_index(layout)] = true;
                if args[1] & 1 != 0 {
                    self.input.layout = layout;
                    if self.input.cursor.is_none() {
                        self.input.delivery_layout = layout;
                    }
                    self.input.dead = None;
                }
                layout.handle()
            }
            UnloadKeyboardLayout => {
                let layout = self.input_layout(args[0])?;
                if layout == self.input.layout || !self.input.loaded[layout_index(layout)] {
                    return Err(User32Error32::Win32(5));
                }
                self.input.loaded[layout_index(layout)] = false;
                1
            }
            GetKeyboardType => match args[0] {
                0 => 4,
                1 => 0,
                2 => 12,
                _ => 0,
            },
            GetKeyNameTextA | GetKeyNameTextW => {
                if args[2] as i32 <= 0 {
                    return Ok(User32Outcome32::Return(0));
                }
                let mut scan = ((args[0] >> 16) & 0x7f) as u16;
                if scan == 0x45 {
                    scan = if args[0] & (1 << 24) != 0 { 0x45 } else { 0xc5 };
                } else if args[0] & (1 << 24) != 0 {
                    scan |= 0x80;
                }
                if args[0] & (1 << 25) != 0 {
                    scan = match scan {
                        0x36 => 0x2a,
                        0x9d => 0x1d,
                        0xb8 => 0x38,
                        _ => scan,
                    };
                }
                let name = keyboard::key_name(scan);
                let mut bytes = [0; 128];
                let mut count = 0;
                for character in name.chars().take((args[2] as usize - 1).min(63)) {
                    if api.wide() {
                        bytes[count * 2..count * 2 + 2]
                            .copy_from_slice(&(character as u16).to_le_bytes());
                    } else {
                        bytes[count] =
                            keyboard::encode_ansi(self.input.layout, character).unwrap_or(b'?');
                    }
                    count += 1;
                }
                memory.write(
                    GuestAddress(args[1]),
                    &bytes[..(count + 1) * if api.wide() { 2 } else { 1 }],
                )?;
                count as u32
            }
            ToAscii | ToAsciiEx | ToUnicode | ToUnicodeEx => {
                self.translate_key_query(api, args, memory)?
            }
            VkKeyScanA | VkKeyScanW | VkKeyScanExA | VkKeyScanExW => {
                let layout = if args.len() == 2 {
                    self.input_layout(args[1])?
                } else {
                    self.input.layout
                };
                let character = if api.wide() {
                    char::from_u32(args[0] & 0xffff)
                } else {
                    Some(keyboard::decode_ansi(layout, args[0] as u8))
                };
                character
                    .and_then(|character| vk_key_scan(layout, character))
                    .map_or(u32::MAX, u32::from)
            }
            _ => return self.pointer_query(api, args, memory, context),
        };
        Ok(User32Outcome32::Return(value))
    }

    pub(super) fn input_layout(&self, handle: u32) -> Result<KeyboardLayout32, User32Error32> {
        if handle == 0 {
            return Ok(self.input.layout);
        }
        KeyboardLayout32::from_handle(handle)
            .filter(|&layout| self.input.loaded[layout_index(layout)])
            .ok_or(User32Error32::Win32(6))
    }
}

fn layout_index(layout: KeyboardLayout32) -> usize {
    usize::from(layout == KeyboardLayout32::TurkishQ)
}

fn vk_key_scan(layout: KeyboardLayout32, character: char) -> Option<u16> {
    for modifiers in [0u16, 1, 2, 6, 7] {
        for key in 1..=255 {
            if keyboard::character(
                layout,
                key,
                modifiers & 1 != 0,
                false,
                modifiers & 2 != 0,
                modifiers & 4 != 0,
            ) == Some(character)
            {
                return Some(u16::from(key) | (modifiers << 8));
            }
        }
    }
    None
}

impl User32Input32 {
    pub(super) fn translation_keys(&self, context: User32CallContext32) -> [u8; 256] {
        let mut keys = self.queued;
        if self.cursor.is_none() {
            keys[0x10] |= u8::from(context.shift) << 7;
            keys[0x11] |= u8::from(context.control) << 7;
            keys[0x14] |= u8::from(context.caps_lock);
        }
        keys
    }
}
