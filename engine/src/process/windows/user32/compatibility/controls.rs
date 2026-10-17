use super::super::memory;
use super::*;
use crate::{
    memory::GuestAddress,
    windows::{controls::ControlKind32, window::WindowHandle32},
};
impl User32Context32 {
    pub(super) fn control_utility_call(
        &mut self,
        api: User32Api32,
        a: &[u32],
        m: &mut AddressSpace,
        c: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        use User32Api32::*;
        let h = WindowHandle32::try_from(a[0])?;
        self.windows().get(h)?;
        if matches!(
            api,
            GetScrollInfo
                | SetScrollInfo
                | GetScrollRange
                | SetScrollRange
                | GetScrollPos
                | SetScrollPos
                | ShowScrollBar
        ) {
            return self.scroll_utility_call(api, a, m);
        }
        if matches!(api, GetNextDlgTabItem | GetNextDlgGroupItem) {
            if api == GetNextDlgGroupItem {
                return Err(User32Error32::Win32(120));
            }
            let previous = if a[1] == 0 {
                None
            } else {
                Some(WindowHandle32::try_from(a[1])?)
            };
            return Ok(User32Outcome32::Return(super::super::raw(
                self.state.dialog_tab_item(h, previous, a[2] != 0)?,
            )));
        }
        if api == IsDialogMessageA {
            return Err(User32Error32::Win32(120));
        }
        if api == CheckRadioButton {
            if a[1] > a[2] {
                return Err(User32Error32::Win32(87));
            }
            let children: Vec<_> = self
                .state
                .related_windows(h, true)
                .filter(|&w| {
                    self.state
                        .record(w)
                        .is_ok_and(|d| (a[1]..=a[2]).contains(&d.menu))
                })
                .collect();
            for child in children {
                if self
                    .state
                    .standard_kind(self.state.record(child)?.class_atom)
                    == Some(ControlKind32::Button)
                {
                    self.state.control(child)?.check =
                        u32::from(self.state.record(child)?.menu == a[3]);
                    self.state.invalidate_control(child)?;
                }
            }
            return Ok(User32Outcome32::Return(1));
        }
        let Some(child) = self.state.dialog_item(h, a[1])? else {
            return Ok(User32Outcome32::Return(0));
        };
        let value = match api {
            SendDlgItemMessageA => return self.send_message(&[child.get(), a[2], a[3], a[4]], c),
            GetDlgItemTextA => return self.send_message(&[child.get(), 0xd, a[3], a[2]], c),
            SetDlgItemTextA => return self.send_message(&[child.get(), 0xc, 0, a[2]], c),
            CheckDlgButton => return self.send_message(&[child.get(), 0xf1, a[2], 0], c),
            IsDlgButtonChecked => return self.send_message(&[child.get(), 0xf0, 0, 0], c),
            SetDlgItemInt => {
                if self
                    .state
                    .standard_kind(self.state.record(child)?.class_atom)
                    .is_none()
                {
                    return Err(User32Error32::Win32(50));
                }
                let text = if a[3] != 0 {
                    (a[2] as i32).to_string()
                } else {
                    a[2].to_string()
                };
                self.state.set_text(child, text.encode_utf16().collect())?;
                self.state.invalidate_control(child)?;
                1
            }
            GetDlgItemInt => {
                if self
                    .state
                    .standard_kind(self.state.record(child)?.class_atom)
                    .is_none()
                {
                    return Err(User32Error32::Win32(50));
                }
                let text = String::from_utf16_lossy(self.state.text(child)?);
                let text = text.trim_start_matches([' ', '\t', '\r', '\n']);
                let signed = a[3] != 0;
                let negative = text.starts_with('-');
                let mut s = text;
                if s.starts_with(['+', '-']) {
                    s = &s[1..];
                }
                let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
                let parsed = digits.parse::<u64>().ok().filter(|&v| {
                    if signed {
                        v <= if negative { 2147483648 } else { 2147483647 }
                    } else {
                        !negative && v <= u64::from(u32::MAX)
                    }
                });
                let result = parsed.map(|v| {
                    if negative {
                        (v as u32).wrapping_neg()
                    } else {
                        v as u32
                    }
                });
                if a[2] != 0 {
                    m.write(
                        GuestAddress(a[2]),
                        &u32::from(result.is_some()).to_le_bytes(),
                    )?;
                }
                result.unwrap_or(0)
            }
            _ => return Err(User32Error32::Win32(120)),
        };
        Ok(User32Outcome32::Return(value))
    }

    fn scroll_utility_call(
        &mut self,
        api: User32Api32,
        a: &[u32],
        m: &mut AddressSpace,
    ) -> Result<User32Outcome32, User32Error32> {
        use User32Api32::*;
        if a[1] > 3 {
            return Err(User32Error32::Win32(87));
        }
        if api == ShowScrollBar {
            return Err(User32Error32::Win32(120));
        }
        if a[1] == 3 {
            return Err(User32Error32::Win32(87));
        }
        let h = WindowHandle32::try_from(a[0])?;
        let control = a[1] == 2;
        if control
            && self.state.standard_kind(self.state.record(h)?.class_atom)
                != Some(ControlKind32::ScrollBar)
        {
            return Err(User32Error32::Win32(1400));
        }
        let at = self
            .compatibility
            .scroll
            .iter()
            .position(|(w, b, _)| *w == a[0] && *b == a[1]);
        let mut s = if control {
            let q = self.state.control_state(h)?.scroll;
            Scroll32 {
                min: q.0,
                max: q.1,
                pos: q.2,
                page: q.3,
                track: q.2,
            }
        } else {
            at.map_or(Scroll32::default(), |i| self.compatibility.scroll[i].2)
        };
        let value = match api {
            GetScrollPos => s.pos as u32,
            GetScrollRange => {
                m.write_spans(&[
                    (GuestAddress(a[2]), &s.min.to_le_bytes()),
                    (GuestAddress(a[3]), &s.max.to_le_bytes()),
                ])?;
                1
            }
            GetScrollInfo => {
                let mut q = read_scroll_info(m, a[2])?;
                if q[1] & !0x1f != 0 {
                    return Err(User32Error32::Win32(87));
                }
                if q[1] & 1 != 0 {
                    q[2] = s.min as u32;
                    q[3] = s.max as u32;
                }
                if q[1] & 2 != 0 {
                    q[4] = s.page;
                }
                if q[1] & 4 != 0 {
                    q[5] = s.pos as u32;
                }
                if q[1] & 16 != 0 && q.len() == 7 {
                    q[6] = s.track as u32;
                }
                m.write(
                    GuestAddress(a[2]),
                    &q.into_iter().flat_map(u32::to_le_bytes).collect::<Vec<_>>(),
                )?;
                return Ok(User32Outcome32::Return(1));
            }
            SetScrollInfo | SetScrollRange | SetScrollPos => {
                let old = s.pos;
                if api == SetScrollInfo {
                    let q = read_scroll_info(m, a[2])?;
                    if q[1] & !0x1f != 0 {
                        return Err(User32Error32::Win32(87));
                    }
                    if q[1] & 1 != 0 {
                        s.min = q[2] as i32;
                        s.max = q[3] as i32;
                    }
                    if q[1] & 2 != 0 {
                        s.page = q[4];
                    }
                    if q[1] & 4 != 0 {
                        s.pos = q[5] as i32;
                    }
                } else if api == SetScrollRange {
                    s.min = a[2] as i32;
                    s.max = a[3] as i32;
                } else {
                    s.pos = a[2] as i32;
                }
                if s.max < s.min {
                    s.min = 0;
                    s.max = 0;
                }
                let span = (i64::from(s.max) - i64::from(s.min) + 1) as u64;
                s.page = s.page.min(span.min(u64::from(u32::MAX)) as u32);
                let upper = (i64::from(s.max) - i64::from(s.page.saturating_sub(1)))
                    .max(i64::from(s.min)) as i32;
                s.pos = s.pos.clamp(s.min, upper);
                s.track = s.pos;
                if control {
                    self.state.control(h)?.scroll = (s.min, s.max, s.pos, s.page);
                } else if let Some(i) = at {
                    self.compatibility.scroll[i].2 = s;
                } else {
                    if self.compatibility.scroll.len() >= 4096 {
                        return Err(User32Error32::Capacity);
                    }
                    self.compatibility
                        .scroll
                        .try_reserve(1)
                        .map_err(|_| User32Error32::Allocation)?;
                    self.compatibility.scroll.push((a[0], a[1], s));
                }
                if api == SetScrollInfo {
                    s.pos as u32
                } else if api == SetScrollRange {
                    1
                } else {
                    old as u32
                }
            }
            _ => return Err(User32Error32::Win32(120)),
        };
        Ok(User32Outcome32::Return(value))
    }
}

fn read_scroll_info(memory: &AddressSpace, address: u32) -> Result<Vec<u32>, User32Error32> {
    let prefix: [u32; 6] = super::super::memory::words(memory, address)?;
    if !matches!(prefix[0], 24 | 28) {
        return Err(User32Error32::Win32(87));
    }
    let mut result = prefix.to_vec();
    if prefix[0] == 28 {
        let [track] = super::super::memory::words(
            memory,
            address.checked_add(24).ok_or(User32Error32::Win32(87))?,
        )?;
        result.push(track);
    }
    Ok(result)
}
