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
    pub relative: RelativeTotals,
    pub cursor: (i32, i32),
    pub focused: bool,
    pub captured: bool,
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
            relative: RelativeTotals::default(),
            cursor: (0, 0),
            focused: false,
            captured: false,
            focus_generation: 0,
            capture_generation: 0,
            controllers: [ControllerState32::default(); MAX_CONTROLLERS],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind32 {
    Physical(InputKind),
    Cursor { x: i32, y: i32 },
    Controller { slot: u8, object: u16, value: u32 },
    Connection { slot: u8, connected: bool },
    Window { focus: u32, capture: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceEvent32 {
    pub sequence: u64,
    pub timestamp_ns: u64,
    pub kind: SourceKind32,
}
impl Default for SourceEvent32 {
    fn default() -> Self {
        Self {
            sequence: 0,
            timestamp_ns: 0,
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
