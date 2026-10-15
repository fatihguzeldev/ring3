use super::{Acquisition32, DirectInputDevice32};
use crate::input::{
    InputKind,
    dinput::{DeviceKind32, format::DeviceObject32},
    source::{InputSource32, SourceEvent32, SourceKind32},
};
impl DirectInputDevice32 {
    pub fn synchronize(
        &mut self,
        source: &InputSource32,
        active_window: u32,
        origin_ns: u64,
    ) -> bool {
        if self.acquisition != Acquisition32::Acquired {
            return false;
        }
        let state = source.snapshot();
        if self.will_lose(source, active_window) {
            self.acquisition = Acquisition32::Lost;
            self.buffer.clear();
            self.overflow = false;
            self.snapshot = *state;
            return true;
        }
        let mut changed = false;
        let mut events = [SourceEvent32::default(); 32];
        loop {
            let read = source
                .read(self.cursor, &mut events)
                .expect("current input source cursor");
            if read.overflow {
                self.overflow = true;
            }
            self.cursor = read.next;
            for event in events[..read.count].iter().copied() {
                changed |= self.accept(event, origin_ns);
            }
            if read.count < events.len() {
                break;
            }
        }
        self.snapshot = *state;
        changed
    }
    fn accept(&mut self, event: SourceEvent32, origin: u64) -> bool {
        let mut changes = [None; 3];
        match (self.description.kind, event.kind) {
            (DeviceKind32::Keyboard, SourceKind32::Physical(InputKind::Key { code, pressed })) => {
                if self.snapshot.keys[usize::from(code)] == pressed {
                    return false;
                }
                self.snapshot.keys[usize::from(code)] = pressed;
                changes[0] = Some((DeviceObject32::Key(code), u32::from(pressed) * 0x80));
            }
            (
                DeviceKind32::Mouse,
                SourceKind32::Physical(InputKind::Button { button, pressed }),
            ) => {
                if self.snapshot.buttons[usize::from(button)] == pressed {
                    return false;
                }
                self.snapshot.buttons[usize::from(button)] = pressed;
                changes[0] = Some((
                    DeviceObject32::Button(u16::from(button)),
                    u32::from(pressed) * 0x80,
                ));
            }
            (
                DeviceKind32::Mouse,
                SourceKind32::Physical(InputKind::Relative { dx, dy, wheel }),
            ) => {
                self.snapshot.relative.dx = self.snapshot.relative.dx.saturating_add(i64::from(dx));
                self.snapshot.relative.dy = self.snapshot.relative.dy.saturating_add(i64::from(dy));
                self.snapshot.relative.wheel = self
                    .snapshot
                    .relative
                    .wheel
                    .saturating_add(i64::from(wheel));
                for (index, value) in [dx, dy, wheel].into_iter().enumerate() {
                    if value != 0 {
                        let object = DeviceObject32::Axis {
                            index: index as u8,
                            aspect: 0,
                        };
                        changes[index] = Some((
                            object,
                            if self.relative() {
                                value as u32
                            } else {
                                self.value(object, &self.snapshot)
                            },
                        ));
                    }
                }
            }
            (DeviceKind32::Mouse, SourceKind32::Cursor { x, y }) => {
                let old = self.snapshot.cursor;
                self.snapshot.cursor = (x, y);
                if !self.relative() {
                    for (index, (value, previous)) in
                        [(x, old.0), (y, old.1)].into_iter().enumerate()
                    {
                        if value != previous {
                            changes[index] = Some((
                                DeviceObject32::Axis {
                                    index: index as u8,
                                    aspect: 0,
                                },
                                value as u32,
                            ));
                        }
                    }
                }
            }
            (
                DeviceKind32::Controller(slot),
                SourceKind32::Controller {
                    slot: target,
                    object,
                    value,
                },
            ) if slot == target => {
                let object = match object {
                    0..=31 => DeviceObject32::Axis {
                        index: (object % 8) as u8,
                        aspect: (object / 8) as u8,
                    },
                    0x100..=0x103 => DeviceObject32::Pov((object - 0x100) as u8),
                    0x200..=0x27f => DeviceObject32::Button(object - 0x200),
                    _ => return false,
                };
                let value = match object {
                    DeviceObject32::Axis { index, aspect } => {
                        let at = usize::from(index) + usize::from(aspect) * 8;
                        let old = self.snapshot.controllers[usize::from(slot)].axes[at];
                        self.snapshot.controllers[usize::from(slot)].axes[at] = value as i32;
                        let current = if aspect == 0 {
                            self.axes[usize::from(index)].map(value as i32)
                        } else {
                            value as i32
                        };
                        if self.relative() {
                            let previous = if aspect == 0 {
                                self.axes[usize::from(index)].map(old)
                            } else {
                                old
                            };
                            current.saturating_sub(previous) as u32
                        } else {
                            current as u32
                        }
                    }
                    _ => value,
                };
                changes[0] = Some((object, value));
            }
            _ => return false,
        }
        let mut changed = false;
        for (object, value) in changes.into_iter().flatten() {
            changed |= self
                .format
                .as_ref()
                .is_some_and(|format| format.offset_for(object).is_some());
            self.emit(object, value, event, origin);
        }
        changed
    }
}
impl DirectInputDevice32 {
    pub fn would_notify(&self, source: &InputSource32, active_window: u32) -> bool {
        if self.acquisition != Acquisition32::Acquired {
            return false;
        }
        if self.will_lose(source, active_window) {
            return true;
        }
        let state = source.snapshot();
        let level = self.cooperative.unwrap();
        if self.cursor.epoch != source.tail().epoch
            || level.foreground
                && (!state.focused
                    || state.focus_generation != self.snapshot.focus_generation
                    || active_window != level.window)
            || matches!(self.description.kind,DeviceKind32::Controller(slot) if !state.controllers[usize::from(slot)].connected)
        {
            return true;
        }
        let mut cursor = self.cursor;
        let mut snapshot = self.snapshot;
        let mut events = [SourceEvent32::default(); 32];
        loop {
            let read = source
                .read(cursor, &mut events)
                .expect("current source cursor");
            cursor = read.next;
            for event in &events[..read.count] {
                if matches!(event.kind,SourceKind32::Window {focus,..} if level.foreground&&focus!=level.window)
                {
                    return true;
                }
                let object = match (self.description.kind, event.kind) {
                    (
                        DeviceKind32::Keyboard,
                        SourceKind32::Physical(InputKind::Key { code, pressed }),
                    ) => {
                        let previous = snapshot.keys[usize::from(code)];
                        snapshot.keys[usize::from(code)] = pressed;
                        if previous == pressed {
                            continue;
                        }
                        Some(DeviceObject32::Key(code))
                    }
                    (
                        DeviceKind32::Mouse,
                        SourceKind32::Physical(InputKind::Button { button, pressed }),
                    ) => {
                        let previous = snapshot.buttons[usize::from(button)];
                        snapshot.buttons[usize::from(button)] = pressed;
                        if previous == pressed {
                            continue;
                        }
                        Some(DeviceObject32::Button(u16::from(button)))
                    }
                    (
                        DeviceKind32::Mouse,
                        SourceKind32::Physical(InputKind::Relative { dx, dy, wheel }),
                    ) => {
                        if [dx, dy, wheel]
                            .into_iter()
                            .enumerate()
                            .any(|(index, value)| {
                                value != 0
                                    && self.format.as_ref().is_some_and(|format| {
                                        format
                                            .offset_for(DeviceObject32::Axis {
                                                index: index as u8,
                                                aspect: 0,
                                            })
                                            .is_some()
                                    })
                            })
                        {
                            return true;
                        }
                        None
                    }
                    (DeviceKind32::Mouse, SourceKind32::Cursor { x, y }) if !self.relative() => {
                        let previous = snapshot.cursor;
                        snapshot.cursor = (x, y);
                        if [(x, previous.0), (y, previous.1)]
                            .into_iter()
                            .enumerate()
                            .any(|(index, (value, previous))| {
                                value != previous
                                    && self.format.as_ref().is_some_and(|format| {
                                        format
                                            .offset_for(DeviceObject32::Axis {
                                                index: index as u8,
                                                aspect: 0,
                                            })
                                            .is_some()
                                    })
                            })
                        {
                            return true;
                        }
                        None
                    }
                    (
                        DeviceKind32::Controller(slot),
                        SourceKind32::Controller {
                            slot: target,
                            object,
                            ..
                        },
                    ) if slot == target => match object {
                        0..=31 => Some(DeviceObject32::Axis {
                            index: (object % 8) as u8,
                            aspect: (object / 8) as u8,
                        }),
                        0x100..=0x103 => Some(DeviceObject32::Pov((object - 0x100) as u8)),
                        0x200..=0x27f => Some(DeviceObject32::Button(object - 0x200)),
                        _ => None,
                    },
                    _ => None,
                };
                if object.is_some_and(|object| {
                    self.format
                        .as_ref()
                        .is_some_and(|format| format.offset_for(object).is_some())
                }) {
                    return true;
                }
            }
            if read.count < events.len() {
                break;
            }
        }
        false
    }
}
impl DirectInputDevice32 {
    pub fn will_lose(&self, source: &InputSource32, active_window: u32) -> bool {
        if self.acquisition != Acquisition32::Acquired {
            return false;
        }
        let level = self.cooperative.unwrap();
        let state = source.snapshot();
        if self.cursor.epoch != source.tail().epoch
            || level.foreground
                && (!state.focused
                    || state.focus_generation != self.snapshot.focus_generation
                    || active_window != level.window)
            || matches!(self.description.kind,DeviceKind32::Controller(slot) if !state.controllers[usize::from(slot)].connected)
        {
            return true;
        }
        if !level.foreground && !matches!(self.description.kind, DeviceKind32::Controller(_)) {
            return false;
        }
        let mut cursor = self.cursor;
        let mut events = [SourceEvent32::default(); 32];
        loop {
            let read = source
                .read(cursor, &mut events)
                .expect("current source cursor");
            cursor = read.next;
            if events[..read.count].iter().any(|event|matches!(event.kind,SourceKind32::Window {focus,..} if level.foreground&&focus!=level.window)||matches!((self.description.kind,event.kind),(DeviceKind32::Controller(slot),SourceKind32::Connection {slot:target,connected:false}) if slot==target)) {
                return true;
            }
            if read.count < events.len() {
                break;
            }
        }
        false
    }
}
