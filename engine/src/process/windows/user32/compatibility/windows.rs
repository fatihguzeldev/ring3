use super::super::memory;
use super::*;
use crate::{
    memory::GuestAddress,
    windows::{
        user32::{Point32, Rect32},
        window::{WindowHandle32, WindowMessage32},
    },
};
impl User32Context32 {
    pub(super) fn window_utility_call(
        &mut self,
        api: User32Api32,
        a: &[u32],
        m: &mut AddressSpace,
        c: User32CallContext32,
    ) -> Result<User32Outcome32, User32Error32> {
        use User32Api32::*;
        if matches!(api, BringWindowToTop | SetForegroundWindow) {
            return Err(User32Error32::Win32(120));
        }
        if api == SetActiveWindow && a[0] == 0 {
            return self.set_active_window(None, c);
        }
        if api == FindWindowA {
            let class = if a[0] == 0 {
                None
            } else {
                Some(super::super::classes::class_name(m, a[0], false, c.ansi)?)
            };
            let title = if a[1] == 0 {
                None
            } else {
                Some(memory::string(m, a[1], false, 32767, c.ansi)?)
            };
            let found = self
                .state
                .all_windows()
                .filter(|&h| Some(h) != self.compatibility.desktop)
                .find(|&h| {
                    self.state
                        .record(h)
                        .is_ok_and(|d| d.style & 0x4000_0000 == 0)
                        && class.as_ref().is_none_or(|(name, atom)| {
                            self.state.window_class(h).is_ok_and(|k| {
                                atom.is_some_and(|atom| k.atom == atom)
                                    || String::from_utf16_lossy(name)
                                        .eq_ignore_ascii_case(&String::from_utf16_lossy(&k.name))
                            })
                        })
                        && title.as_ref().is_none_or(|title| {
                            self.state.text(h).is_ok_and(|text| {
                                String::from_utf16_lossy(title).to_lowercase()
                                    == String::from_utf16_lossy(text).to_lowercase()
                            })
                        })
                });
            return Ok(User32Outcome32::Return(super::super::raw(found)));
        }
        if api == WindowFromPoint {
            let p = Point32 {
                x: a[0] as i32,
                y: a[1] as i32,
            };
            let found = self
                .state
                .all_windows()
                .filter(|&h| Some(h) != self.compatibility.desktop)
                .rev()
                .find(|&h| {
                    self.windows()
                        .get(h)
                        .is_ok_and(|r| r.description.visible && r.description.enabled)
                        && self.state.record(h).is_ok_and(|d| {
                            p.x >= d.outer.left
                                && p.x < d.outer.right
                                && p.y >= d.outer.top
                                && p.y < d.outer.bottom
                        })
                });
            let found = found.or_else(|| {
                self.compatibility.desktop.filter(|&desktop| {
                    self.state.record(desktop).is_ok_and(|data| {
                        p.x >= data.outer.left
                            && p.x < data.outer.right
                            && p.y >= data.outer.top
                            && p.y < data.outer.bottom
                    })
                })
            });
            return Ok(User32Outcome32::Return(super::super::raw(found)));
        }
        if api == GetTopWindow
            && (a[0] == 0
                || self
                    .compatibility
                    .desktop
                    .is_some_and(|desktop| desktop.get() == a[0]))
        {
            let h = self
                .state
                .all_windows()
                .filter(|&h| Some(h) != self.compatibility.desktop)
                .rev()
                .find(|&h| {
                    self.state
                        .record(h)
                        .is_ok_and(|d| d.style & 0x4000_0000 == 0)
                });
            return Ok(User32Outcome32::Return(super::super::raw(h)));
        }
        if api == GetClassInfoA {
            let (name, atom) = super::super::classes::class_name(m, a[1], false, c.ansi)?;
            let class = self
                .state
                .class(if a[0] == 0 { c.instance } else { a[0] }, &name, atom)?;
            if !class.menu_name.is_empty() {
                return Err(User32Error32::Win32(50));
            }
            let values = [
                class.style,
                class.procedure.0,
                0,
                class.extra_bytes,
                class.instance,
                class.icon,
                class.cursor,
                class.background_brush,
                class.menu_id.map_or(0, u32::from),
                a[1],
            ];
            let bytes: Vec<u8> = values.into_iter().flat_map(u32::to_le_bytes).collect();
            m.write(GuestAddress(a[2]), &bytes)?;
            return Ok(User32Outcome32::Return(1));
        }
        let h = WindowHandle32::try_from(a[0])?;
        let data = self.state.record(h)?;
        let record = self.windows().get(h)?;
        let value =
            match api {
                GetParent => {
                    if data.style & 0xc000_0000 != 0 {
                        super::super::raw(data.parent)
                    } else {
                        0
                    }
                }
                SetParent => {
                    self.owned(a[0], c.thread_id)?;
                    let parent = if a[1] == 0 {
                        None
                    } else {
                        Some(self.owned(a[1], c.thread_id)?)
                    };
                    if parent.is_some_and(|p| p == h || self.state.destroy_tree_contains(p, h)) {
                        return Err(User32Error32::Win32(87));
                    }
                    let mut changed = data;
                    changed.parent = parent;
                    self.state.update(changed)?;
                    super::super::raw(data.parent)
                }
                IsChild => {
                    let child = WindowHandle32::try_from(a[1]).ok();
                    u32::from(child.is_some_and(|child| {
                        child != h && self.state.is_descendant_window(child, h)
                    }))
                }
                GetClassNameA => {
                    let class = self.state.window_class(h)?;
                    let (mut bytes, _) = c
                        .ansi
                        .encode(&class.name, false)
                        .map_err(|_| User32Error32::Win32(1113))?;
                    if a[2] == 0 {
                        return Ok(User32Outcome32::Return(0));
                    }
                    bytes.truncate(a[2].saturating_sub(1) as usize);
                    let count = bytes.len() as u32;
                    bytes.push(0);
                    m.write(GuestAddress(a[1]), &bytes)?;
                    count
                }
                GetClassLongA => {
                    let class = self.state.window_class(h)?;
                    match a[1] as i32 {
                        -32 => u32::from(class.atom),
                        -26 => class.style,
                        -24 => class.procedure.0,
                        -16 => class.instance,
                        -18 => class.extra_bytes,
                        -20 => 0,
                        -14 => class.icon,
                        -12 => class.cursor,
                        -10 => class.background_brush,
                        -34 => class.small_icon,
                        -8 if class.menu_name.is_empty() => class.menu_id.map_or(0, u32::from),
                        _ => return Err(User32Error32::Win32(1413)),
                    }
                }
                GetWindowThreadProcessId => {
                    if a[1] != 0 {
                        m.write(GuestAddress(a[1]), &c.process_id.to_le_bytes())?;
                    }
                    record.description.thread_id
                }
                IsWindowVisible => {
                    let mut next = Some(h);
                    let mut visible = true;
                    for _ in 0..=self.windows().live_windows() {
                        let Some(w) = next else { break };
                        let d = self.state.record(w)?;
                        visible &= self.windows().get(w)?.description.visible;
                        next = if d.style & 0x4000_0000 != 0 {
                            d.parent
                        } else {
                            None
                        };
                    }
                    u32::from(visible)
                }
                IsWindowEnabled => u32::from(record.description.enabled),
                IsIconic => u32::from(data.style & 0x2000_0000 != 0),
                IsZoomed => u32::from(data.style & 0x0100_0000 != 0),
                GetWindow | GetTopWindow => {
                    let cmd = if api == GetTopWindow { 5 } else { a[1] };
                    let mut siblings: Vec<_> = self
                        .state
                        .all_windows()
                        .filter(|&w| Some(w) != self.compatibility.desktop)
                        .filter(|&w| {
                            self.state.record(w).is_ok_and(|d| {
                                if data.style & 0x4000_0000 != 0 {
                                    d.style & 0x4000_0000 != 0 && d.parent == data.parent
                                } else {
                                    d.style & 0x4000_0000 == 0
                                }
                            })
                        })
                        .collect();
                    siblings.reverse();
                    let i = siblings.iter().position(|&w| w == h);
                    let target = match cmd {
                        0 => siblings.first().copied(),
                        1 => siblings.last().copied(),
                        2 => i.and_then(|i| siblings.get(i + 1).copied()),
                        3 => i
                            .and_then(|i| i.checked_sub(1))
                            .and_then(|i| siblings.get(i).copied()),
                        4 => {
                            if data.style & 0x4000_0000 == 0 {
                                data.parent
                            } else {
                                None
                            }
                        }
                        5 if Some(h) == self.compatibility.desktop => {
                            self.state.all_windows().rev().find(|&window| {
                                Some(window) != self.compatibility.desktop
                                    && self
                                        .state
                                        .record(window)
                                        .is_ok_and(|data| data.style & 0x4000_0000 == 0)
                            })
                        }
                        5 => self.state.related_windows(h, true).next_back(),
                        6 => self
                            .state
                            .related_windows(h, false)
                            .rev()
                            .find(|&w| self.windows().get(w).is_ok_and(|r| r.description.enabled))
                            .or(Some(h)),
                        _ => return Err(User32Error32::Win32(87)),
                    };
                    super::super::raw(target)
                }
                GetLastActivePopup => {
                    let popup = self.windows().active().filter(|&active| {
                        active == h || self.state.destroy_tree_contains(active, h)
                    });
                    super::super::raw(popup.or(Some(h)))
                }
                GetPropA | SetPropA | RemovePropA => {
                    if a[1] <= 0xffff {
                        return Err(User32Error32::Win32(87));
                    }
                    let key = memory::string(m, a[1], false, 255, c.ansi)?;
                    return self.window_property(api, a[0], key, a.get(2).copied().unwrap_or(0));
                }
                EnableWindow => {
                    self.owned(a[0], c.thread_id)?;
                    let disabled = !record.description.enabled;
                    let enabled = a[1] != 0;
                    if record.description.enabled == enabled {
                        return Ok(User32Outcome32::Return(u32::from(disabled)));
                    }
                    self.check_continuation()?;
                    let mut changed = data;
                    if enabled {
                        changed.style &= !0x0800_0000
                    } else {
                        changed.style |= 0x0800_0000
                    }
                    let mut r = record.description;
                    r.enabled = enabled;
                    self.state.windows_mut().update_description(h, r)?;
                    self.state.update(changed)?;
                    return self.sequence(
                        [
                            Some(WindowMessage32 {
                                window: h,
                                procedure: record.description.procedure,
                                message: 0xa,
                                wparam: u32::from(enabled),
                                lparam: 0,
                            }),
                            None,
                            None,
                            None,
                            None,
                            None,
                        ],
                        u32::from(disabled),
                    );
                }
                SetActiveWindow => return self.set_active_window(Some(h), c),
                ShowOwnedPopups => {
                    if self.state.related_windows(h, false).next().is_some() {
                        return Err(User32Error32::Win32(120));
                    }
                    1
                }
                GetWindowPlacement => {
                    let [length] = memory::words(m, a[1])?;
                    if length != 44 {
                        return Err(User32Error32::Win32(87));
                    }
                    let r = data.outer;
                    let vals = [
                        44,
                        0,
                        if data.style & 0x2000_0000 != 0 {
                            2
                        } else if data.style & 0x0100_0000 != 0 {
                            3
                        } else {
                            1
                        },
                        u32::MAX,
                        u32::MAX,
                        u32::MAX,
                        u32::MAX,
                        r.left as u32,
                        r.top as u32,
                        r.right as u32,
                        r.bottom as u32,
                    ];
                    m.write(
                        GuestAddress(a[1]),
                        &vals
                            .into_iter()
                            .flat_map(u32::to_le_bytes)
                            .collect::<Vec<_>>(),
                    )?;
                    1
                }
                SetWindowPlacement => {
                    let vals: [u32; 11] = memory::words(m, a[1])?;
                    if vals[0] != 44 {
                        return Err(User32Error32::Win32(87));
                    }
                    let current_show = if data.style & 0x2000_0000 != 0 {
                        2
                    } else if data.style & 0x0100_0000 != 0 {
                        3
                    } else if record.description.visible {
                        1
                    } else {
                        0
                    };
                    if vals[1] != 0 || vals[2] != current_show {
                        return Err(User32Error32::Win32(50));
                    }
                    let r = Rect32 {
                        left: vals[7] as i32,
                        top: vals[8] as i32,
                        right: vals[9] as i32,
                        bottom: vals[10] as i32,
                    };
                    let extent = r.extent()?;
                    return self.move_window(
                        User32Api32::SetWindowPos,
                        &[
                            a[0],
                            0,
                            r.left as u32,
                            r.top as u32,
                            extent.width,
                            extent.height,
                            0x14,
                        ],
                        m,
                        c,
                    );
                }
                EnumChildWindows => return Err(User32Error32::Win32(120)),
                _ => return Err(User32Error32::Win32(120)),
            };
        Ok(User32Outcome32::Return(value))
    }
}

impl User32Context32 {
    pub(in crate::process::windows::user32) fn window_property(
        &mut self,
        api: User32Api32,
        window: u32,
        name: Vec<u16>,
        value: u32,
    ) -> Result<User32Outcome32, User32Error32> {
        self.state.record(WindowHandle32::try_from(window)?)?;
        let key = crate::windows::text::ordinal_key(&name);
        let at = self
            .compatibility
            .properties
            .iter()
            .position(|(handle, key2, _)| *handle == window && *key2 == key);
        let result = match api {
            User32Api32::GetPropA => at.map_or(0, |i| self.compatibility.properties[i].2),
            User32Api32::RemovePropA => at.map_or(0, |i| self.compatibility.properties.remove(i).2),
            User32Api32::SetPropA => {
                if let Some(i) = at {
                    self.compatibility.properties[i].2 = value;
                } else {
                    if self.compatibility.properties.len() >= 4096 {
                        return Err(User32Error32::Capacity);
                    }
                    self.compatibility
                        .properties
                        .try_reserve(1)
                        .map_err(|_| User32Error32::Allocation)?;
                    self.compatibility.properties.push((window, key, value));
                }
                1
            }
            _ => return Err(User32Error32::InvalidRequest),
        };
        Ok(User32Outcome32::Return(result))
    }
}
