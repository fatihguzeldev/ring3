use super::*;
fn inside(point: Point32, rect: Rect32) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}
impl User32Context32 {
    pub(in crate::process::windows::user32::common) fn trackbar_input(
        &mut self,
        window: WindowHandle32,
        args: &[u32],
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        let record = self.windows().get(window)?;
        if !record.description.enabled {
            return Ok(User32Outcome32::Return(0));
        }
        let extent = record.description.extent;
        let style = self.state.record(window)?.style;
        let vertical = style & 2 != 0;
        let value = self.trackbar(window)?;
        let point = Point32 {
            x: i32::from(args[3] as i16),
            y: i32::from((args[3] >> 16) as i16),
        };
        let channel = value.channel(extent.width as i32, extent.height as i32, vertical);
        let thumb = value.thumb(extent.width as i32, extent.height as i32, vertical);
        let coordinate = if vertical { point.y } else { point.x };
        let axis = value.coordinate(value.position, channel, vertical);
        let (position, code, key, arm, release, focus) = match args[1] {
            0x100 => {
                let (delta, code) = match args[2] {
                    37 | 38 => (-i64::from(value.line), 0),
                    39 | 40 => (i64::from(value.line), 1),
                    33 => (-i64::from(value.page()), 2),
                    34 => (i64::from(value.page()), 3),
                    36 => (i64::from(value.minimum) - i64::from(value.position), 6),
                    35 => (i64::from(value.maximum) - i64::from(value.position), 7),
                    _ => return Ok(User32Outcome32::Return(0)),
                };
                let delta = if style & 0x400 != 0 && matches!(args[2], 38 | 40) {
                    -delta
                } else {
                    delta
                };
                (
                    value.clamp(i64::from(value.position) + delta),
                    code,
                    true,
                    None,
                    false,
                    None,
                )
            }
            0x101 if value.key_active && matches!(args[2], 33..=40) => {
                return self.start_numeric(
                    window,
                    NumericAction32::End,
                    0,
                    0,
                    None,
                    false,
                    None,
                    ram,
                    call,
                );
            }
            0x201 | 0x203 => {
                if !inside(
                    point,
                    Rect32 {
                        left: 0,
                        top: 0,
                        right: extent.width as i32,
                        bottom: extent.height as i32,
                    },
                ) {
                    return Ok(User32Outcome32::Return(0));
                }
                if style & 0x80 == 0 && inside(point, thumb) {
                    (
                        value.position,
                        5,
                        false,
                        Some(Arm32::Thumb),
                        false,
                        Some(window),
                    )
                } else {
                    let step = if coordinate < axis {
                        -value.page()
                    } else {
                        value.page()
                    };
                    (
                        value.clamp(i64::from(value.position) + i64::from(step)),
                        if step < 0 { 2 } else { 3 },
                        false,
                        Some(Arm32::Page(point)),
                        false,
                        Some(window),
                    )
                }
            }
            0x200 if value.dragging && self.windows().capture() == Some(window) => (
                value.value_at(coordinate, channel, vertical),
                5,
                false,
                None,
                false,
                None,
            ),
            0x202 if self.windows().capture() == Some(window) => {
                if value.dragging {
                    (
                        value.value_at(coordinate, channel, vertical),
                        4,
                        false,
                        None,
                        true,
                        None,
                    )
                } else {
                    return self.start_numeric(
                        window,
                        NumericAction32::End,
                        0,
                        0,
                        None,
                        true,
                        None,
                        ram,
                        call,
                    );
                }
            }
            0x113
                if args[2] == 1
                    && self.windows().capture() == Some(window)
                    && value.pressed.is_some() =>
            {
                let (x, y) = value.pressed.unwrap();
                let target = value.value_at(if vertical { y } else { x }, channel, vertical);
                if target == value.position {
                    return Ok(User32Outcome32::Return(0));
                }
                let step = if target < value.position {
                    -value.page()
                } else {
                    value.page()
                };
                let proposed = value.clamp(i64::from(value.position) + i64::from(step));
                let position = if target < value.position {
                    proposed.max(target)
                } else {
                    proposed.min(target)
                };
                (
                    position,
                    if step < 0 { 2 } else { 3 },
                    false,
                    None,
                    false,
                    None,
                )
            }
            0x215 | 0x1f if value.dragging || value.pressed.is_some() => {
                return self.start_numeric(
                    window,
                    NumericAction32::Cancel,
                    0,
                    0,
                    None,
                    true,
                    None,
                    ram,
                    call,
                );
            }
            _ => return Ok(User32Outcome32::Return(0)),
        };
        self.start_numeric(
            window,
            NumericAction32::Track {
                position,
                code,
                key,
            },
            0,
            0,
            arm,
            release,
            focus,
            ram,
            call,
        )
    }
    pub(in crate::process::windows::user32::common) fn updown_input(
        &mut self,
        window: WindowHandle32,
        args: &[u32],
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        let record = self.windows().get(window)?;
        if !record.description.enabled {
            return Ok(User32Outcome32::Return(0));
        }
        let extent = record.description.extent;
        let style = self.state.record(window)?.style;
        let value = self.updown(window)?;
        let point = Point32 {
            x: i32::from(args[3] as i16),
            y: i32::from((args[3] >> 16) as i16),
        };
        let (delta, arm, focus) = match args[1] {
            0x100 if style & 0x20 != 0 => {
                let direction = match args[2] {
                    38 => 1,
                    40 => -1,
                    37 if style & 0x40 != 0 => -1,
                    39 if style & 0x40 != 0 => 1,
                    _ => return Ok(User32Outcome32::Return(0)),
                };
                (direction, None, None)
            }
            0x101 if value.key_active => {
                return self.start_numeric(
                    window,
                    NumericAction32::End,
                    0,
                    0,
                    None,
                    false,
                    None,
                    ram,
                    call,
                );
            }
            0x201 | 0x203 => {
                if !inside(
                    point,
                    Rect32 {
                        left: 0,
                        top: 0,
                        right: extent.width as i32,
                        bottom: extent.height as i32,
                    },
                ) {
                    return Ok(User32Outcome32::Return(0));
                }
                let direction = if style & 0x40 != 0 {
                    if point.x < extent.width as i32 / 2 {
                        -1
                    } else {
                        1
                    }
                } else if point.y < extent.height as i32 / 2 {
                    1
                } else {
                    -1
                };
                let focus = self.updown_buddy(window)?.filter(|&buddy| {
                    self.state.record(buddy).is_ok_and(|data| {
                        matches!(
                            self.state.standard_kind(data.class_atom),
                            Some(
                                crate::windows::controls::ControlKind32::Edit
                                    | crate::windows::controls::ControlKind32::ListBox
                            )
                        )
                    })
                });
                (direction, Some(Arm32::Spin(direction)), focus)
            }
            0x113
                if args[2] == 1
                    && self.windows().capture() == Some(window)
                    && value.pressed.is_some() =>
            {
                let (direction, start) = value.pressed.unwrap();
                let seconds = call.now_ns.saturating_sub(start) / 1_000_000_000;
                (direction * value.step(seconds), None, None)
            }
            0x202 if value.pressed.is_some() => {
                return self.start_numeric(
                    window,
                    NumericAction32::End,
                    0,
                    0,
                    None,
                    true,
                    None,
                    ram,
                    call,
                );
            }
            0x215 | 0x1f if value.pressed.is_some() => {
                return self.start_numeric(
                    window,
                    NumericAction32::Cancel,
                    0,
                    0,
                    None,
                    true,
                    None,
                    ram,
                    call,
                );
            }
            _ => return Ok(User32Outcome32::Return(0)),
        };
        let position = value.position;
        self.start_numeric(
            window,
            NumericAction32::Spin {
                position,
                delta: Some(delta),
                range: None,
                base: None,
            },
            0,
            0,
            arm,
            false,
            focus,
            ram,
            call,
        )
    }
    pub(in crate::process::windows::user32) fn numeric_buddy_key_target(
        &self,
        window: WindowHandle32,
        args: &[u32],
    ) -> Option<WindowHandle32> {
        if !matches!(args[1], 0x100 | 0x101) || !matches!(args[2], 38 | 40) {
            return None;
        }
        let parent = self.state.record(window).ok()?.parent?;
        self.state.related_windows(parent,true).find(|&candidate|self.state.record(candidate).is_ok_and(|data|data.style&0x20!=0)&&matches!(self.state.common_state(candidate),Ok(State::UpDown(value))if value.buddy==Some(window)))
    }
}
