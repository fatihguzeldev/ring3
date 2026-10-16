use super::{
    RawInputRuntime32,
    types::{KEYBOARD, MOUSE, RawPacket32},
};
use crate::{input::InputKind, windows::user32::User32Error32 as E};

pub(crate) struct PreparedRawPacket32(pub(super) RawPacket32);
impl PreparedRawPacket32 {
    pub(crate) fn packet(&self) -> &RawPacket32 {
        &self.0
    }
}
fn word(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
impl RawInputRuntime32 {
    pub(crate) fn prepare_packet(
        &self,
        kind: InputKind,
        key: u16,
        system: bool,
        focus: u32,
        physical_focus: bool,
        owner: impl Fn(u32) -> Option<u32>,
    ) -> Result<Option<PreparedRawPacket32>, E> {
        let keyboard = matches!(kind, InputKind::Key { .. });
        if !matches!(
            kind,
            InputKind::Key { .. } | InputKind::Button { .. } | InputKind::Relative { .. }
        ) || !physical_focus
        {
            return Ok(None);
        }
        let Some(registration) = self.registration(if keyboard { 6 } else { 2 }) else {
            return Ok(None);
        };
        let foreground = owner(focus).is_some();
        let window = if registration.window == 0 {
            focus
        } else {
            registration.window
        };
        let Some(thread) = owner(window) else {
            return Ok(None);
        };
        if !foreground && registration.flags & (0x100 | 0x1000) == 0 {
            return Ok(None);
        }
        let sink = u32::from(!foreground);
        if self.packets.len() == self.limit {
            return Err(E::Win32(1816));
        }
        self.next.checked_add(4).ok_or(E::Capacity)?;
        let mut bytes = [0; 40];
        let length = if keyboard { 32 } else { 40 };
        word(&mut bytes, 0, u32::from(keyboard));
        word(&mut bytes, 4, length as u32);
        word(&mut bytes, 8, if keyboard { KEYBOARD } else { MOUSE });
        word(&mut bytes, 12, sink);
        match kind {
            InputKind::Key { code, pressed } => {
                bytes[16..18].copy_from_slice(&(code & 0x7f).to_le_bytes());
                let flags = u16::from(!pressed)
                    | if code == 0xc5 {
                        4
                    } else {
                        u16::from(code & 0x80 != 0 || code == 0x45) << 1
                    };
                bytes[18..20].copy_from_slice(&flags.to_le_bytes());
                bytes[22..24].copy_from_slice(&key.to_le_bytes());
                word(
                    &mut bytes,
                    24,
                    if system {
                        if pressed { 0x104 } else { 0x105 }
                    } else if pressed {
                        0x100
                    } else {
                        0x101
                    },
                );
            }
            InputKind::Button { button, pressed } => {
                if button >= 5 {
                    return Ok(None);
                }
                let flags = 1u16 << (u16::from(button) * 2 + u16::from(!pressed));
                bytes[20..22].copy_from_slice(&flags.to_le_bytes());
            }
            InputKind::Relative { dx, dy, wheel } => {
                if wheel != 0 {
                    bytes[20..22].copy_from_slice(&0x400u16.to_le_bytes());
                    bytes[22..24].copy_from_slice(&(wheel as i16).to_le_bytes());
                }
                word(&mut bytes, 28, dx as u32);
                word(&mut bytes, 32, dy as u32);
            }
            _ => unreachable!(),
        }
        Ok(Some(PreparedRawPacket32(RawPacket32 {
            id: self.next,
            window,
            thread,
            bytes,
            length,
            queued: true,
        })))
    }
    pub(crate) fn commit_packet(&mut self, prepared: PreparedRawPacket32) {
        assert_eq!(prepared.0.id, self.next);
        self.next += 4;
        self.packets.push(prepared.0);
    }
}
