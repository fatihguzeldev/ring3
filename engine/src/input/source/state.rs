use crate::input::{
    BUTTON_COUNT, InputError, InputKind, KEY_COUNT, RelativeTotals, TimedInputEvent,
};

pub const MAX_CONTROLLERS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControllerState32 {
    pub connected: bool,
    pub axes: [i32; 32],
    pub pov: [u32; 4],
    pub buttons: [bool; 128],
}
impl Default for ControllerState32 {
    fn default() -> Self {
        Self {
            connected: false,
            axes: [0; 32],
            pov: [u32::MAX; 4],
            buttons: [false; 128],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceSnapshot32 {
    pub keys: [bool; KEY_COUNT],
    pub presses: [u64; KEY_COUNT],
    pub buttons: [bool; BUTTON_COUNT],
    pub button_presses: [u64; BUTTON_COUNT],
    pub relative: RelativeTotals,
    pub cursor: (i32, i32),
    pub cursor_bounds: Option<(i32, i32, i32, i32)>,
    pub focused: bool,
    pub captured: bool,
    pub mouse_exclusive: bool,
    pub no_windows_key: bool,
    pub focus_generation: u64,
    pub capture_generation: u64,
    pub controllers: [ControllerState32; MAX_CONTROLLERS],
}
impl Default for SourceSnapshot32 {
    fn default() -> Self {
        Self {
            keys: [false; KEY_COUNT],
            presses: [0; KEY_COUNT],
            buttons: [false; BUTTON_COUNT],
            button_presses: [0; BUTTON_COUNT],
            relative: RelativeTotals::default(),
            cursor: (0, 0),
            cursor_bounds: None,
            focused: false,
            captured: false,
            mouse_exclusive: false,
            no_windows_key: false,
            focus_generation: 0,
            capture_generation: 0,
            controllers: [ControllerState32::default(); MAX_CONTROLLERS],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind32 {
    CursorPolicy {
        x: i32,
        y: i32,
        bounds: Option<(i32, i32, i32, i32)>,
    },
    Physical(InputKind),
    Cursor {
        x: i32,
        y: i32,
    },
    Controller {
        slot: u8,
        object: u16,
        value: u32,
    },
    Connection {
        slot: u8,
        connected: bool,
    },
    Window {
        focus: u32,
        capture: u32,
    },
    Policy {
        mouse_exclusive: bool,
        no_windows_key: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceEvent32 {
    pub sequence: u64,
    pub timestamp_ns: u64,
    pub input_sequence: u32,
    pub kind: SourceKind32,
}
impl Default for SourceEvent32 {
    fn default() -> Self {
        Self {
            sequence: 0,
            timestamp_ns: 0,
            input_sequence: 0,
            kind: SourceKind32::Physical(InputKind::Focus { focused: false }),
        }
    }
}

pub(super) fn physical(
    state: &mut SourceSnapshot32,
    event: TimedInputEvent,
    mut emit: impl FnMut(SourceKind32),
) -> Result<(), InputError> {
    match event.kind {
        InputKind::Key { code, pressed } => {
            let key = state
                .keys
                .get_mut(code as usize)
                .ok_or(InputError::InvalidEvent)?;
            if pressed && !state.focused {
                return Err(InputError::NotFocused);
            }
            if pressed && !*key {
                state.presses[code as usize] = state.presses[code as usize]
                    .checked_add(1)
                    .ok_or(InputError::Exhausted)?;
            }
            *key = pressed;
        }
        InputKind::Button { button, pressed } => {
            let value = state
                .buttons
                .get_mut(button as usize)
                .ok_or(InputError::InvalidEvent)?;
            if pressed && !state.focused {
                return Err(InputError::NotFocused);
            }
            if pressed && !*value {
                state.button_presses[button as usize] = state.button_presses[button as usize]
                    .checked_add(1)
                    .ok_or(InputError::Exhausted)?;
            }
            *value = pressed;
        }
        InputKind::Relative { dx, dy, wheel } => {
            if !state.focused {
                return Err(InputError::NotFocused);
            }
            state.relative.dx = state
                .relative
                .dx
                .checked_add(i64::from(dx))
                .ok_or(InputError::Exhausted)?;
            state.relative.dy = state
                .relative
                .dy
                .checked_add(i64::from(dy))
                .ok_or(InputError::Exhausted)?;
            state.relative.wheel = state
                .relative
                .wheel
                .checked_add(i64::from(wheel))
                .ok_or(InputError::Exhausted)?;
            state.cursor.0 = state.cursor.0.saturating_add(dx);
            state.cursor.1 = state.cursor.1.saturating_add(dy);
            state.cursor = clamp_cursor(state.cursor, state.cursor_bounds);
        }
        InputKind::Focus { focused } => {
            if !focused {
                if state.focused {
                    state.focus_generation = state
                        .focus_generation
                        .checked_add(1)
                        .ok_or(InputError::Exhausted)?;
                }
                if state.captured {
                    state.capture_generation = state
                        .capture_generation
                        .checked_add(1)
                        .ok_or(InputError::Exhausted)?;
                }
                for (index, key) in state.keys.iter_mut().enumerate() {
                    if *key {
                        *key = false;
                        emit(SourceKind32::Physical(InputKind::Key {
                            code: index as u16,
                            pressed: false,
                        }));
                    }
                }
                for (index, button) in state.buttons.iter_mut().enumerate() {
                    if *button {
                        *button = false;
                        emit(SourceKind32::Physical(InputKind::Button {
                            button: index as u8,
                            pressed: false,
                        }));
                    }
                }
                state.captured = false;
            }
            state.focused = focused;
        }
        InputKind::Capture { captured } => {
            if captured && !state.focused {
                return Err(InputError::NotFocused);
            }
            if !captured && state.captured {
                state.capture_generation = state
                    .capture_generation
                    .checked_add(1)
                    .ok_or(InputError::Exhausted)?;
            }
            state.captured = captured;
        }
    }
    emit(SourceKind32::Physical(event.kind));
    Ok(())
}

pub(super) fn clamp_cursor(point: (i32, i32), bounds: Option<(i32, i32, i32, i32)>) -> (i32, i32) {
    if let Some((left, top, right, bottom)) = bounds {
        (
            point.0.clamp(left, right.saturating_sub(1).max(left)),
            point.1.clamp(top, bottom.saturating_sub(1).max(top)),
        )
    } else {
        point
    }
}
