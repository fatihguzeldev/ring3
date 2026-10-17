use super::super::memory;
use super::*;
use crate::{memory::GuestAddress, windows::text::CodePage32};

fn read_byte(m: &AddressSpace, p: u32) -> Result<u8, User32Error32> {
    let mut b = [0];
    m.read(GuestAddress(p), &mut b)?;
    Ok(b[0])
}
fn append(out: &mut Vec<u8>, s: &[u8]) -> Result<(), User32Error32> {
    if out.len() + s.len() > 1023 {
        return Err(User32Error32::Win32(122));
    }
    out.extend_from_slice(s);
    Ok(())
}
impl User32Context32 {
    pub(super) fn text_utility_call(
        &mut self,
        api: User32Api32,
        a: &[u32],
        m: &mut AddressSpace,
        c: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        use User32Api32::*;
        let value = match api {
            CharNextA => {
                let first = read_byte(m, a[0])?;
                if first == 0 {
                    a[0]
                } else {
                    let step = if c.ansi.is_lead_byte(first)
                        && read_byte(m, a[0].checked_add(1).ok_or(User32Error32::Win32(87))?)? != 0
                    {
                        2
                    } else {
                        1
                    };
                    a[0].checked_add(step).ok_or(User32Error32::Win32(87))?
                }
            }
            CharUpperA => {
                if a[0] <= 0xffff {
                    let units = c
                        .ansi
                        .decode(&[a[0] as u8], false)
                        .map_err(|_| User32Error32::Win32(1113))?;
                    let upper = String::from_utf16_lossy(&units).to_uppercase();
                    let (bytes, _) = c
                        .ansi
                        .encode(&upper.encode_utf16().collect::<Vec<_>>(), false)
                        .map_err(|_| User32Error32::Win32(1113))?;
                    bytes.first().copied().map_or(0, u32::from)
                } else {
                    let units = memory::string(m, a[0], false, 32767, c.ansi)?;
                    let upper: Vec<u16> = String::from_utf16_lossy(&units)
                        .chars()
                        .map(|ch| {
                            let mut up = ch.to_uppercase();
                            let one = up.next().unwrap();
                            if up.next().is_none() { one } else { ch }
                        })
                        .collect::<String>()
                        .encode_utf16()
                        .collect();
                    let (mut bytes, _) = c
                        .ansi
                        .encode(&upper, false)
                        .map_err(|_| User32Error32::Win32(1113))?;
                    bytes.push(0);
                    m.write(GuestAddress(a[0]), &bytes)?;
                    a[0]
                }
            }
            CharToOemA | OemToCharA => {
                let from = if api == CharToOemA {
                    c.ansi
                } else {
                    CodePage32::Oem437
                };
                let to = if api == CharToOemA {
                    CodePage32::Oem437
                } else {
                    c.ansi
                };
                let units = memory::string(m, a[0], false, 32767, from)?;
                let (mut bytes, _) = to
                    .encode(&units, false)
                    .map_err(|_| User32Error32::Win32(1113))?;
                bytes.push(0);
                m.write(GuestAddress(a[1]), &bytes)?;
                1
            }
            WvsprintfA => return self.format_user_string(a[0], a[1], a[2], m, c.ansi),
            _ => return Err(User32Error32::Win32(120)),
        };
        Ok(User32Outcome32::Return(value))
    }

    pub(in crate::process::windows::user32) fn format_user_string(
        &mut self,
        destination: u32,
        format: u32,
        mut arguments: u32,
        m: &mut AddressSpace,
        cp: CodePage32,
    ) -> Result<User32Outcome32, User32Error32> {
        let units = memory::string(m, format, false, 32767, cp)?;
        let (f, _) = cp
            .encode(&units, false)
            .map_err(|_| User32Error32::Win32(1113))?;
        let mut out = Vec::new();
        let mut i = 0;
        while i < f.len() {
            if f[i] != b'%' {
                append(&mut out, &f[i..i + 1])?;
                i += 1;
                continue;
            }
            i += 1;
            if f.get(i) == Some(&b'%') {
                append(&mut out, b"%")?;
                i += 1;
                continue;
            }
            let mut left = false;
            let mut zero = false;
            let mut alternate = false;
            while let Some(flag) = f.get(i) {
                match flag {
                    b'-' => left = true,
                    b'0' => zero = true,
                    b'#' => alternate = true,
                    _ => break,
                }
                i += 1;
            }
            let mut width = 0usize;
            while f.get(i).is_some_and(u8::is_ascii_digit) {
                width = width
                    .saturating_mul(10)
                    .saturating_add((f[i] - b'0') as usize)
                    .min(1024);
                i += 1;
            }
            let mut precision = None;
            if f.get(i) == Some(&b'.') {
                i += 1;
                let mut p = 0usize;
                while f.get(i).is_some_and(u8::is_ascii_digit) {
                    p = p
                        .saturating_mul(10)
                        .saturating_add((f[i] - b'0') as usize)
                        .min(1024);
                    i += 1;
                }
                precision = Some(p);
            }
            let mut modifier = 0;
            if f.get(i).is_some_and(|b| matches!(b, b'h' | b'l' | b'w')) {
                modifier = f[i];
                i += 1;
            }
            let spec = *f.get(i).ok_or(User32Error32::Win32(87))?;
            i += 1;
            let [word] = memory::words(m, arguments)?;
            arguments = arguments.checked_add(4).ok_or(User32Error32::Win32(87))?;
            let mut value = match spec {
                b'd' | b'i' => (if modifier == b'h' {
                    i32::from(word as u16 as i16)
                } else {
                    word as i32
                })
                .to_string()
                .into_bytes(),
                b'u' => (if modifier == b'h' {
                    u32::from(word as u16)
                } else {
                    word
                })
                .to_string()
                .into_bytes(),
                b'x' | b'p' => {
                    let value = if modifier == b'h' {
                        u32::from(word as u16)
                    } else {
                        word
                    };
                    format!("{value:x}").into_bytes()
                }
                b'X' => {
                    let value = if modifier == b'h' {
                        u32::from(word as u16)
                    } else {
                        word
                    };
                    format!("{value:X}").into_bytes()
                }
                b'c' | b'C' => {
                    let wide =
                        modifier == b'l' || modifier == b'w' || spec == b'C' && modifier != b'h';
                    if wide {
                        cp.encode(&[word as u16], false)
                            .map_err(|_| User32Error32::Win32(1113))?
                            .0
                    } else {
                        vec![word as u8]
                    }
                }
                b's' | b'S' => {
                    if word == 0 {
                        b"(null)".to_vec()
                    } else {
                        let wide = modifier == b'l'
                            || modifier == b'w'
                            || spec == b'S' && modifier != b'h';
                        if let Some(limit) = precision {
                            let mut bytes = Vec::new();
                            for index in 0..limit {
                                let at = word
                                    .checked_add((index * if wide { 2 } else { 1 }) as u32)
                                    .ok_or(User32Error32::Win32(87))?;
                                let mut unit = [0u8; 2];
                                m.read(GuestAddress(at), &mut unit[..if wide { 2 } else { 1 }])?;
                                if unit[0] == 0 && (!wide || unit[1] == 0) {
                                    break;
                                }
                                bytes.extend_from_slice(&unit[..if wide { 2 } else { 1 }]);
                            }
                            if wide {
                                let units: Vec<u16> = bytes
                                    .chunks_exact(2)
                                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                                    .collect();
                                cp.encode(&units, false)
                                    .map_err(|_| User32Error32::Win32(1113))?
                                    .0
                            } else {
                                bytes
                            }
                        } else {
                            let text = memory::string(m, word, wide, 32767, cp)?;
                            cp.encode(&text, false)
                                .map_err(|_| User32Error32::Win32(1113))?
                                .0
                        }
                    }
                }
                _ => return Err(User32Error32::Win32(87)),
            };
            if matches!(spec, b's' | b'S') {
                if let Some(p) = precision {
                    value.truncate(p);
                }
            }
            if alternate && matches!(spec, b'x' | b'X') && word != 0 {
                let prefix = if spec == b'X' { b"0X" } else { b"0x" };
                value.splice(0..0, prefix.iter().copied());
            }
            if matches!(spec, b'x' | b'X' | b'p' | b'u' | b'd' | b'i') {
                if let Some(p) = precision {
                    let prefix = if value.starts_with(b"0x") || value.starts_with(b"0X") {
                        2
                    } else {
                        usize::from(value.first() == Some(&b'-'))
                    };
                    let pad = p.saturating_sub(value.len() - prefix);
                    value.splice(prefix..prefix, std::iter::repeat_n(b'0', pad));
                }
                if spec == b'p' {
                    width = width.max(8);
                    zero = true;
                }
            }
            let padding = width.saturating_sub(value.len());
            if !left {
                if zero {
                    let prefix = if value.starts_with(b"0x") || value.starts_with(b"0X") {
                        2
                    } else {
                        usize::from(value.first() == Some(&b'-'))
                    };
                    append(&mut out, &value[..prefix])?;
                    value.drain(..prefix);
                }
                append(&mut out, &vec![if zero { b'0' } else { b' ' }; padding])?;
            }
            append(&mut out, &value)?;
            if left {
                append(&mut out, &vec![b' '; padding])?;
            }
        }
        let n = out.len() as u32;
        out.push(0);
        m.write(GuestAddress(destination), &out)?;
        Ok(User32Outcome32::Return(n))
    }
}
