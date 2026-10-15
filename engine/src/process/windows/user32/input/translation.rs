use super::super::{User32CallContext32, User32Context32, User32Outcome32, memory};
use crate::{
    input::keyboard::{self, KeyboardLayout32},
    memory::{AddressSpace, GuestAddress},
    windows::user32::{Message32, User32Api32, User32Error32},
};

#[derive(Clone, Copy)]
pub(super) struct Translation32 {
    pub characters: [char; 2],
    pub count: usize,
    pub dead: bool,
    pub next: Option<char>,
}

pub(super) fn translate(
    layout: KeyboardLayout32,
    key: u8,
    keys: &[u8; 256],
    previous: Option<char>,
) -> Translation32 {
    let shift = keys[0x10] & 0x80 != 0;
    let control =
        keys[0x11] & 0x80 != 0 || keys[0xa5] & 0x80 != 0 && layout == KeyboardLayout32::TurkishQ;
    let alt = keys[0x12] & 0x80 != 0 || keys[0xa5] & 0x80 != 0;
    let mut result = Translation32 {
        characters: ['\0'; 2],
        count: 0,
        dead: false,
        next: previous,
    };
    let Some(character) =
        keyboard::character(layout, key, shift, keys[0x14] & 1 != 0, control, alt)
    else {
        return result;
    };
    if keyboard::dead_key(layout, key, shift, control, alt) {
        result.characters[0] = character;
        result.count = 1;
        result.dead = true;
        result.next = Some(character);
        return result;
    }
    result.next = None;
    if let Some(accent) = previous {
        if let Some(composed) = keyboard::compose_dead(accent, character) {
            result.characters[0] = composed;
            result.count = 1;
        } else {
            result.characters = [accent, character];
            result.count = 2;
        }
    } else {
        result.characters[0] = character;
        result.count = 1;
    }
    result
}

impl User32Context32 {
    pub(super) fn translate_key_query(
        &mut self,
        api: User32Api32,
        args: &[u32],
        memory: &mut AddressSpace,
    ) -> Result<u32, User32Error32> {
        let unicode = matches!(api, User32Api32::ToUnicode | User32Api32::ToUnicodeEx);
        let ex = matches!(api, User32Api32::ToUnicodeEx | User32Api32::ToAsciiEx);
        let flags = args[if unicode { 5 } else { 4 }];
        if flags & !(if unicode { 7 } else { 1 }) != 0 {
            return Err(User32Error32::Win32(87));
        }
        if unicode && args[4] as i32 <= 0 {
            return Err(User32Error32::Win32(87));
        }
        let layout = if ex {
            self.input_layout(args[if unicode { 6 } else { 5 }])?
        } else {
            self.input.layout
        };
        let mut keys = [0; 256];
        memory.read(GuestAddress(args[2]), &mut keys)?;
        if args[0] > 255 || args[1] & 0x8000 != 0 && flags & 2 == 0 {
            return Ok(0);
        }
        let translation = translate(layout, args[0] as u8, &keys, self.input.dead);
        if translation.count == 0 {
            return Ok(0);
        }
        let count = translation
            .count
            .min(if unicode { args[4] as usize } else { 2 });
        let mut bytes = [0; 4];
        for (index, &character) in translation.characters[..count].iter().enumerate() {
            if unicode {
                bytes[index * 2..index * 2 + 2].copy_from_slice(&(character as u16).to_le_bytes());
            } else {
                bytes[index] = keyboard::encode_ansi(layout, character).unwrap_or(b'?');
            }
        }
        memory.write(
            GuestAddress(args[3]),
            &bytes[..if unicode { count * 2 } else { 2 }],
        )?;
        if flags & 4 == 0 {
            self.input.dead = translation.next;
        }
        Ok(if translation.dead {
            u32::MAX
        } else {
            count as u32
        })
    }

    pub(in crate::process::windows::user32) fn translate_message(
        &mut self,
        address: u32,
        memory: &AddressSpace,
        context: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        let message = memory::message(memory, address)?;
        if !matches!(message.message, 0x100 | 0x101 | 0x104 | 0x105) {
            return Ok(User32Outcome32::Return(0));
        }
        if matches!(message.message, 0x101 | 0x105) || message.wparam > 255 {
            return Ok(User32Outcome32::Return(1));
        }
        let keys = self.input.translation_keys(context);
        let translation = translate(
            self.input.layout,
            message.wparam as u8,
            &keys,
            self.input.dead,
        );
        if translation.count == 0 {
            return Ok(User32Outcome32::Return(1));
        }
        let (thread, unicode) = if message.window == 0 {
            (context.thread_id, true)
        } else {
            let data = self
                .state
                .record(crate::windows::window::WindowHandle32::try_from(
                    message.window,
                )?)?;
            (
                self.windows().get(data.handle)?.description.thread_id,
                data.unicode,
            )
        };
        self.state.can_post(translation.count)?;
        for &character in &translation.characters[..translation.count] {
            self.state
                .post(Message32 {
                    message: match (message.message == 0x104, translation.dead) {
                        (false, false) => 0x102,
                        (false, true) => 0x103,
                        (true, false) => 0x106,
                        (true, true) => 0x107,
                    },
                    wparam: if unicode {
                        character as u32
                    } else {
                        u32::from(
                            keyboard::encode_ansi(self.input.layout, character).unwrap_or(b'?'),
                        )
                    },
                    thread_id: thread,
                    ..message
                })
                .expect("translated message capacity checked");
        }
        self.input.dead = translation.next;
        Ok(User32Outcome32::Return(1))
    }
}
