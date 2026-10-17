use super::*;
impl User32Context32 {
    fn updown_list_buddy(&self, buddy: WindowHandle32) -> Result<bool, E> {
        Ok(self
            .state
            .standard_kind(self.state.record(buddy)?.class_atom)
            == Some(crate::windows::controls::ControlKind32::ListBox))
    }
    fn numeric_scroll(&self, call: NumericCall32, end: bool) -> Result<Option<[u32; 4]>, E> {
        let Some(parent) = call
            .parent
            .filter(|&parent| self.windows().get(parent).is_ok())
        else {
            return Ok(None);
        };
        let style = self.state.record(call.window)?.style;
        let (position, code, vertical) = match call.action {
            NumericAction32::Track { code, .. } => (
                self.trackbar(call.window)?.position,
                if end { 8 } else { code },
                style & 2 != 0,
            ),
            NumericAction32::Spin { delta: Some(_), .. } => (
                self.updown(call.window)?.position,
                if end { 8 } else { 4 },
                style & 0x40 == 0,
            ),
            NumericAction32::End => (
                match self.state.common_state(call.window)? {
                    State::Trackbar(value) => value.position,
                    State::UpDown(value) => value.position,
                    _ => return Err(E::InvalidRequest),
                },
                8,
                match self.state.common_state(call.window)? {
                    State::Trackbar(_) => style & 2 != 0,
                    _ => style & 0x40 == 0,
                },
            ),
            _ => return Ok(None),
        };
        let packed = code
            | if matches!(code, 4 | 5) {
                u32::from(position as u16) << 16
            } else {
                0
            };
        Ok(Some([
            parent.get(),
            if vertical { 0x115 } else { 0x114 },
            packed,
            call.window.get(),
        ]))
    }
    pub(in crate::process::windows::user32) fn pending_numeric_message(
        &self,
        token: u64,
    ) -> Result<Option<[u32; 4]>, E> {
        let call = self.numeric_call(token)?;
        if call.child.is_some() {
            return Err(E::InvalidToken);
        }
        if self.windows().get(call.window).is_err() {
            return Ok(None);
        }
        match call.stage {
            Stage32::Notify => Ok(call
                .parent
                .filter(|&parent| self.windows().get(parent).is_ok())
                .map(|parent| {
                    [
                        parent.get(),
                        0x4e,
                        self.state
                            .record(call.window)
                            .expect("same numeric owner")
                            .menu,
                        call.scratch.address.0,
                    ]
                })),
            Stage32::ReadBuddy | Stage32::Buddy => {
                let Some(buddy) = self.updown_buddy(call.window)? else {
                    return Ok(None);
                };
                if call.buddy_list {
                    return Ok(Some([
                        buddy.get(),
                        if matches!(call.stage, Stage32::ReadBuddy) {
                            0x188
                        } else {
                            0x186
                        },
                        if matches!(call.stage, Stage32::ReadBuddy) {
                            0
                        } else {
                            self.updown(call.window)?.position as u32
                        },
                        0,
                    ]));
                }
                Ok(Some([
                    buddy.get(),
                    if matches!(call.stage, Stage32::ReadBuddy) {
                        0xd
                    } else {
                        0xc
                    },
                    if matches!(call.stage, Stage32::ReadBuddy) {
                        64
                    } else {
                        0
                    },
                    call.scratch.address.0 + 64,
                ]))
            }
            Stage32::SelectBuddy => {
                let buddy = if matches!(self.state.common_state(call.window)?, State::UpDown(_)) {
                    self.updown_buddy(call.window)?
                } else {
                    None
                };
                Ok(buddy
                    .filter(|&buddy| {
                        self.state.record(buddy).is_ok_and(|data| {
                            self.state.standard_kind(data.class_atom)
                                == Some(crate::windows::controls::ControlKind32::Edit)
                        })
                    })
                    .map(|buddy| [buddy.get(), 0xb1, 0, u32::MAX]))
            }
            Stage32::Scroll => self.numeric_scroll(call, false),
            Stage32::EndScroll => self.numeric_scroll(call, true),
            _ => Ok(None),
        }
    }
    fn after_numeric_child(call: &mut NumericCall32, kind: Child32, result: u32) {
        call.stage = match kind {
            Child32::Focus => {
                if call.arm.is_some() {
                    Stage32::Capture
                } else {
                    Stage32::Arm
                }
            }
            Child32::Capture => Stage32::Arm,
            Child32::ReadBuddy => {
                if call.buddy_list {
                    call.buddy_selection = result as i32;
                }
                Stage32::Parsed
            }
            Child32::Notify if result != 0 => Stage32::Finish,
            Child32::Notify => Stage32::Apply,
            Child32::Buddy => {
                if matches!(call.action, NumericAction32::Spin { delta: Some(_), .. }) {
                    Stage32::Scroll
                } else {
                    Stage32::Finish
                }
            }
            Child32::Scroll => {
                if call.release {
                    Stage32::EndScroll
                } else {
                    Stage32::Finish
                }
            }
            Child32::EndScroll => {
                if call.release {
                    Stage32::SelectBuddy
                } else {
                    Stage32::Finish
                }
            }
            Child32::SelectBuddy => Stage32::Release,
            Child32::Release => Stage32::Finish,
            Child32::LayoutBuddy => Stage32::LayoutControl,
            Child32::LayoutControl => {
                if call.buddy_text {
                    Stage32::Buddy
                } else {
                    Stage32::Finish
                }
            }
        };
    }
    fn write_updown_buddy(
        &self,
        call: &mut NumericCall32,
        ram: &mut AddressSpace,
    ) -> Result<bool, E> {
        let style = self.state.record(call.window)?.style;
        if style & 2 == 0 {
            return Ok(false);
        }
        let Some(buddy) = self.updown_buddy(call.window)? else {
            return Ok(false);
        };
        call.buddy_wide = self.state.record(buddy)?.unicode;
        call.buddy_list = self.updown_list_buddy(buddy)?;
        if call.buddy_list {
            return Ok(true);
        }
        let value = self.updown(call.window)?;
        let text = value.format(value.position, style & 0x80 == 0);
        let (bytes, _) = memory::encoded_text(64, &text, call.buddy_wide, call.call)?;
        ram.write(GuestAddress(call.scratch.address.0 + 64), &bytes)?;
        Ok(true)
    }
    pub(in crate::process::windows::user32) fn resume_numeric(
        &mut self,
        token: u64,
        ram: &mut AddressSpace,
    ) -> Result<User32Outcome32, E> {
        let mut call = self.numeric_call(token)?;
        call.scratch.check(ram)?;
        call.check_output(ram)?;
        if call.child.is_some() {
            return Err(E::InvalidToken);
        }
        if self.windows().get(call.window).is_err() {
            call.stage = Stage32::Finish;
        }
        match call.stage {
            Stage32::Focus | Stage32::Capture => {
                let focus = matches!(call.stage, Stage32::Focus);
                let focus_target = call
                    .focus
                    .filter(|&target| self.windows().get(target).is_ok());
                let args = [if focus {
                    focus_target.map_or(0, |target| target.get())
                } else {
                    call.window.get()
                }];
                let outcome = self.prepare_arguments(
                    if focus {
                        User32Api32::SetFocus
                    } else {
                        User32Api32::SetCapture
                    },
                    &args,
                    ram,
                    call.call,
                )?;
                let kind = if focus {
                    Child32::Focus
                } else {
                    Child32::Capture
                };
                if let Some(child) = child_token(outcome) {
                    call.child = Some((child, kind));
                    call.completed = None;
                    self.publish_numeric(token, call);
                    return Ok(outcome);
                }
                Self::after_numeric_child(&mut call, kind, 0);
            }
            Stage32::Arm => {
                if matches!(
                    call.action,
                    NumericAction32::Read { .. } | NumericAction32::Spin { delta: Some(_), .. }
                ) && self.state.record(call.window)?.style & 2 != 0
                    && self.updown_buddy(call.window)?.is_some()
                {
                    ram.ensure_transaction_versions(1)?;
                }
                let repeat = call.arm.is_none()
                    && self.windows().capture() == Some(call.window)
                    && match call.action {
                        NumericAction32::Track { code: 2 | 3, .. } => {
                            self.trackbar(call.window)?.pressed.is_some()
                        }
                        NumericAction32::Spin { delta: Some(_), .. } => {
                            self.updown(call.window)?.pressed.is_some()
                        }
                        _ => false,
                    };
                if repeat {
                    self.set_timer(&[call.window.get(), 1, 100, 0], call.call)?;
                }
                if let Some(arm) = call.arm {
                    self.state.check_paint_change()?;
                    if !matches!(arm, Arm32::Thumb) {
                        self.set_timer(&[call.window.get(), 1, 400, 0], call.call)?;
                    }
                    match arm {
                        Arm32::Thumb => {
                            let value = self.trackbar_mut(call.window)?;
                            value.dragging = true;
                            value.pressed = None;
                        }
                        Arm32::Page(point) => {
                            let value = self.trackbar_mut(call.window)?;
                            value.dragging = false;
                            value.pressed = Some((point.x, point.y));
                        }
                        Arm32::Spin(delta) => {
                            self.updown_mut(call.window)?.pressed = Some((delta, call.call.now_ns))
                        }
                    }
                    self.numeric_changed(call.window);
                }
                match call.action {
                    NumericAction32::Track { .. } => call.stage = Stage32::Apply,
                    NumericAction32::Cancel => call.stage = Stage32::Release,
                    NumericAction32::End => {
                        self.state.check_paint_change()?;
                        match self.state.common_state_mut(call.window)? {
                            State::Trackbar(value) => value.key_active = false,
                            State::UpDown(value) => value.key_active = false,
                            _ => return Err(E::InvalidRequest),
                        }
                        call.stage = Stage32::EndScroll;
                    }
                    NumericAction32::Read { .. } | NumericAction32::Spin { delta: Some(_), .. }
                        if self.state.record(call.window)?.style & 2 != 0
                            && self.updown_buddy(call.window)?.is_some() =>
                    {
                        let buddy = self.updown_buddy(call.window)?.unwrap();
                        call.buddy_wide = self.state.record(buddy)?.unicode;
                        call.buddy_list = self.updown_list_buddy(buddy)?;
                        if !call.buddy_list {
                            ram.write(GuestAddress(call.scratch.address.0 + 64), &[0; 128])?;
                        }
                        call.stage = Stage32::ReadBuddy;
                    }
                    NumericAction32::Read { .. } => call.stage = Stage32::Finish,
                    NumericAction32::Spin { delta: Some(_), .. } => call.stage = Stage32::Notify,
                    _ => call.stage = Stage32::Apply,
                }
            }
            Stage32::ReadBuddy
            | Stage32::Buddy
            | Stage32::Notify
            | Stage32::Scroll
            | Stage32::EndScroll
            | Stage32::SelectBuddy => {
                if matches!(call.stage, Stage32::Notify) {
                    let delta = match call.action {
                        NumericAction32::Spin {
                            delta: Some(delta), ..
                        } => delta,
                        _ => return Err(E::InvalidRequest),
                    };
                    let values = [
                        call.window.get(),
                        self.state.record(call.window)?.menu,
                        (-722i32) as u32,
                        call.start as u32,
                        delta as u32,
                    ];
                    ram.write(
                        call.scratch.address,
                        &values
                            .into_iter()
                            .flat_map(u32::to_le_bytes)
                            .collect::<Vec<_>>(),
                    )?;
                }
                if let Some(message) = self.pending_numeric_message(token)? {
                    let kind = match call.stage {
                        Stage32::ReadBuddy => Child32::ReadBuddy,
                        Stage32::Buddy => Child32::Buddy,
                        Stage32::Notify => Child32::Notify,
                        Stage32::Scroll => Child32::Scroll,
                        Stage32::SelectBuddy => Child32::SelectBuddy,
                        _ => Child32::EndScroll,
                    };
                    let outcome = self.send_message(&message, call.call)?;
                    let child = child_token(outcome).ok_or(E::InvalidRequest)?;
                    call.child = Some((child, kind));
                    call.completed = None;
                    self.publish_numeric(token, call);
                    return Ok(outcome);
                }
                let kind = match call.stage {
                    Stage32::ReadBuddy => Child32::ReadBuddy,
                    Stage32::Buddy => Child32::Buddy,
                    Stage32::Notify => Child32::Notify,
                    Stage32::Scroll => Child32::Scroll,
                    Stage32::SelectBuddy => Child32::SelectBuddy,
                    _ => Child32::EndScroll,
                };
                Self::after_numeric_child(&mut call, kind, 0);
            }
            Stage32::Parsed => {
                if call.buddy_list {
                    let value = self.updown(call.window)?;
                    if call.buddy_selection >= 0
                        && value.clamp(i64::from(call.buddy_selection)) == call.buddy_selection
                    {
                        call.start = call.buddy_selection;
                    } else {
                        call.error = true;
                    }
                } else {
                    let text = memory::string(
                        ram,
                        call.scratch.address.0 + 64,
                        call.buddy_wide,
                        63,
                        call.call.ansi,
                    )?;
                    match self.updown(call.window)?.parse(&text) {
                        Some(value) => call.start = value,
                        None => call.error = true,
                    }
                }
                call.stage = if matches!(call.action, NumericAction32::Read { .. }) {
                    Stage32::Finish
                } else {
                    Stage32::Notify
                };
            }
            Stage32::Apply => {
                self.state.check_paint_change()?;
                match call.action {
                    NumericAction32::Track { position, key, .. } => {
                        let value = self.trackbar_mut(call.window)?;
                        value.position = value.clamp(i64::from(position));
                        value.key_active |= key;
                        call.stage = Stage32::Scroll;
                    }
                    NumericAction32::Spin {
                        position,
                        delta,
                        range,
                        base,
                    } => {
                        let delta = if delta.is_some() {
                            Some(memory::words::<1>(ram, call.scratch.address.0 + 16)?[0] as i32)
                        } else {
                            None
                        };
                        let style = self.state.record(call.window)?.style;
                        let value = self.updown(call.window)?;
                        let pos = delta.map_or(position, |delta| {
                            value.delta(call.start, delta, style & 1 != 0)
                        });
                        // stage the real buddy text before publishing the corresponding numeric state.
                        let text = {
                            let value = self.updown(call.window)?;
                            let proposed = if let Some((a, b)) = range {
                                pos.clamp(a.min(b), a.max(b))
                            } else {
                                value.clamp(i64::from(pos))
                            };
                            let base = base.unwrap_or(value.base);
                            crate::windows::comctl32::updown::UpDown32::format_value(
                                proposed,
                                base,
                                style & 0x80 == 0,
                            )
                        };
                        call.buddy_text =
                            style & 2 != 0 && self.updown_buddy(call.window)?.is_some();
                        if call.buddy_text {
                            let buddy = self.updown_buddy(call.window)?.unwrap();
                            call.buddy_wide = self.state.record(buddy)?.unicode;
                            call.buddy_list = self.updown_list_buddy(buddy)?;
                            if !call.buddy_list {
                                let (bytes, _) =
                                    memory::encoded_text(64, &text, call.buddy_wide, call.call)?;
                                ram.write(GuestAddress(call.scratch.address.0 + 64), &bytes)?;
                            }
                        }
                        let value = self.updown_mut(call.window)?;
                        if let Some((a, b)) = range {
                            value.set_range(a, b);
                        }
                        if let Some(base) = base {
                            value.base = base;
                        }
                        value.position = value.clamp(i64::from(pos));
                        value.key_active |= delta.is_some() && call.arm.is_none() && !call.release;
                        call.stage = if style & 2 != 0 && self.updown_buddy(call.window)?.is_some()
                        {
                            Stage32::Buddy
                        } else if delta.is_some() {
                            Stage32::Scroll
                        } else {
                            Stage32::Finish
                        };
                    }
                    NumericAction32::Bind { buddy } => {
                        if let Some(buddy) =
                            buddy.filter(|&buddy| self.windows().get(buddy).is_ok())
                        {
                            let style = self.state.record(call.window)?.style;
                            call.buddy_text = style & 2 != 0;
                            if call.buddy_text {
                                call.buddy_wide = self.state.record(buddy)?.unicode;
                                call.buddy_list = self.updown_list_buddy(buddy)?;
                                let value = self.updown(call.window)?;
                                let text = value.format(value.position, style & 0x80 == 0);
                                let (bytes, _) =
                                    memory::encoded_text(64, &text, call.buddy_wide, call.call)?;
                                if !call.buddy_list {
                                    ram.write(GuestAddress(call.scratch.address.0 + 64), &bytes)?;
                                }
                            }
                        }
                        self.updown_mut(call.window)?.buddy = buddy;
                        call.stage = if buddy.is_some() {
                            Stage32::LayoutBuddy
                        } else {
                            Stage32::Finish
                        };
                    }
                    _ => call.stage = Stage32::Finish,
                }
                self.numeric_changed(call.window);
            }
            Stage32::LayoutBuddy | Stage32::LayoutControl => {
                let style = self.state.record(call.window)?.style;
                let buddy = self.updown_buddy(call.window)?;
                if style & 12 == 0 || buddy.is_none() {
                    call.buddy_text = self.write_updown_buddy(&mut call, ram)?;
                    call.stage = if call.buddy_text {
                        Stage32::Buddy
                    } else {
                        Stage32::Finish
                    };
                } else {
                    let buddy = buddy.unwrap();
                    if call.layout.is_none() {
                        let rect = self.state.record(buddy)?.outer;
                        let own = self.state.record(call.window)?.outer;
                        let width = own
                            .right
                            .saturating_sub(own.left)
                            .max(1)
                            .min(rect.right.saturating_sub(rect.left).max(1));
                        let (target, spin) = if style & 8 != 0 {
                            (
                                Rect32 {
                                    left: rect.left + width,
                                    ..rect
                                },
                                Rect32 {
                                    right: rect.left + width,
                                    ..rect
                                },
                            )
                        } else {
                            (
                                Rect32 {
                                    right: rect.right - width,
                                    ..rect
                                },
                                Rect32 {
                                    left: rect.right - width,
                                    ..rect
                                },
                            )
                        };
                        call.layout = Some((target, spin));
                    }
                    let (target, spin) = call.layout.unwrap();
                    let first = matches!(call.stage, Stage32::LayoutBuddy);
                    let rect = if first { target } else { spin };
                    let handle = if first { buddy } else { call.window };
                    let args = [
                        handle.get(),
                        0,
                        rect.left as u32,
                        rect.top as u32,
                        rect.right.saturating_sub(rect.left).max(0) as u32,
                        rect.bottom.saturating_sub(rect.top).max(0) as u32,
                        0x14,
                    ];
                    let outcome =
                        self.prepare_arguments(User32Api32::SetWindowPos, &args, ram, call.call)?;
                    let kind = if first {
                        Child32::LayoutBuddy
                    } else {
                        Child32::LayoutControl
                    };
                    if let Some(child) = child_token(outcome) {
                        call.child = Some((child, kind));
                        call.completed = None;
                        self.publish_numeric(token, call);
                        return Ok(outcome);
                    }
                    Self::after_numeric_child(&mut call, kind, 0);
                }
            }
            Stage32::Release => {
                self.state.check_paint_change()?;
                self.kill_timer(&[call.window.get(), 1], call.call)?;
                match self.state.common_state_mut(call.window)? {
                    State::Trackbar(value) => {
                        value.dragging = false;
                        value.pressed = None;
                    }
                    State::UpDown(value) => value.pressed = None,
                    _ => return Err(E::InvalidRequest),
                }
                self.numeric_changed(call.window);
                if self.windows().capture() == Some(call.window) {
                    let outcome =
                        self.prepare_arguments(User32Api32::ReleaseCapture, &[], ram, call.call)?;
                    if let Some(child) = child_token(outcome) {
                        call.child = Some((child, Child32::Release));
                        call.completed = None;
                        self.publish_numeric(token, call);
                        return Ok(outcome);
                    }
                }
                call.stage = Stage32::Finish;
            }
            Stage32::Finish => {
                let read = matches!(call.action, NumericAction32::Read { .. })
                    && self.windows().get(call.window).is_ok();
                let changed =
                    read && !call.error && self.updown(call.window)?.position != call.start;
                if changed {
                    self.state.check_paint_change()?;
                }
                let words = call.output.map(|output| crate::memory::WordWrite32 {
                    address: output.address,
                    value: u32::from(call.error),
                });
                ram.unmap_ranges_with_word_writes(&[call.scratch.range()], words.as_slice())?;
                if let NumericAction32::Read { legacy } = call.action {
                    if changed {
                        self.updown_mut(call.window)?.position = call.start;
                        self.numeric_changed(call.window);
                    }
                    call.result = if legacy {
                        u32::from(call.start as u16) | (u32::from(call.error) << 16)
                    } else {
                        call.start as u32
                    };
                }
                self.pending.pop();
                return Ok(User32Outcome32::Return(call.result));
            }
        }
        self.publish_numeric(token, call);
        Ok(User32Outcome32::Wait {
            continuation: token,
        })
    }
    pub(in crate::process::windows::user32) fn complete_numeric_child(
        &mut self,
        child: u64,
        outcome: User32Outcome32,
    ) -> Result<User32Outcome32, E> {
        let User32Outcome32::Return(result) = outcome else {
            return Ok(outcome);
        };
        let Some(pending) = self.pending.last() else {
            return Ok(outcome);
        };
        let Continuation32::Numeric(mut call) = pending.continuation else {
            return Ok(outcome);
        };
        let Some((expected, kind)) = call.child else {
            return Ok(outcome);
        };
        if child != expected {
            return Ok(outcome);
        }
        let token = pending.token;
        call.child = None;
        call.completed = Some(child);
        Self::after_numeric_child(&mut call, kind, result);
        self.publish_numeric(token, call);
        Ok(User32Outcome32::Wait {
            continuation: token,
        })
    }
    pub(in crate::process::windows::user32) fn retry_numeric_completion(
        &self,
        child: u64,
    ) -> Option<User32Outcome32> {
        let pending = self.pending.last()?;
        let Continuation32::Numeric(call) = pending.continuation else {
            return None;
        };
        (call.child.is_none() && call.completed == Some(child)).then_some(User32Outcome32::Wait {
            continuation: pending.token,
        })
    }
}
