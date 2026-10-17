use super::*;
use crate::windows::window::WindowMessage32;

impl User32Context32 {
    pub(super) fn toolbar_input(
        &mut self,
        window: WindowHandle32,
        args: &[u32],
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        let enabled = self.windows().get(window)?.description.enabled;
        let point = Point32 {
            x: i32::from(args[3] as i16),
            y: i32::from((args[3] >> 16) as i16),
        };
        let t = self.toolbar(window)?;
        let old = t.pressed;
        let hit = t
            .hit(point)
            .filter(|i| t.buttons[*i].style & 1 == 0 && t.buttons[*i].state & 4 != 0);
        let mut pressed = old;
        let mut capture = None;
        let mut activate = None;
        match args[1] {
            0x201 if enabled => {
                pressed = hit.map(|i| t.buttons[i].key);
                if pressed.is_some() {
                    capture = Some(Some(window));
                }
            }
            0x200 if old.is_some() => {}
            0x202 => {
                if enabled && self.windows().capture() == Some(window) {
                    activate = hit.filter(|i| Some(t.buttons[*i].key) == old);
                    capture = Some(None);
                }
                pressed = None;
            }
            0x215 | 0x1f | 8 => {
                pressed = None;
                if args[1] != 0x215 && self.windows().capture() == Some(window) {
                    capture = Some(None);
                }
            }
            0xa if args[2] == 0 => {
                pressed = None;
                if self.windows().capture() == Some(window) {
                    capture = Some(None);
                }
            }
            _ => return Ok(User32Outcome32::Return(0)),
        }
        let command = activate.map(|i| t.buttons[i].command);
        self.check_control_transition(window, call.thread_id, None, capture, &[], 1)?;
        let mut events = self.commit_control_transition(window, call.thread_id, None, capture, &[]);
        let t = self.toolbar_mut(window)?;
        t.pressed = pressed;
        if let Some(key) = old
            && let Some(button) = t.buttons.iter_mut().find(|button| button.key == key)
        {
            button.state &= !2;
        }
        if let Some(key) = pressed {
            if let Some(i) = hit.filter(|i| t.buttons[*i].key == key) {
                t.buttons[i].state |= 2;
            }
        }
        if let Some(i) = activate {
            if t.buttons[i].style & 2 != 0 {
                let checked = t.buttons[i].style & 4 != 0 || t.buttons[i].state & 1 == 0;
                t.check_button(i, checked);
            }
        }
        self.state
            .invalidate_control(window)
            .expect("toolbar input preflighted");
        if let Some(command) = command {
            if let Some(parent) = self.state.record(window)?.parent {
                let procedure = self.windows().get(parent)?.description.procedure;
                let slot = events
                    .iter_mut()
                    .find(|event| event.is_none())
                    .expect("capture change leaves command slot");
                *slot = Some(WindowMessage32 {
                    window: parent,
                    procedure,
                    message: 0x111,
                    wparam: command & 0xffff,
                    lparam: window.get(),
                });
            }
        }
        self.control_sequence(events, 0)
    }
    pub(super) fn begin_toolbar_layout(
        &mut self,
        window: WindowHandle32,
        ram: &mut AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, HostError> {
        let record = self.state.record(window).map_err(HostError::User32)?;
        if record.style & 4 != 0 {
            return Ok(User32Outcome32::Return(0));
        }
        let parent = record.parent.ok_or(HostError::User32(E::Win32(1406)))?;
        let parent_width = self
            .windows()
            .get(parent)
            .map_err(E::from)
            .map_err(HostError::User32)?
            .description
            .extent
            .width;
        let height = self.toolbar(window).map_err(HostError::User32)?.size.1 as u32;
        let current = self
            .state
            .client_position(window)
            .map_err(HostError::User32)?;
        let extent = self
            .windows()
            .get(window)
            .map_err(E::from)
            .map_err(HostError::User32)?
            .description
            .extent;
        let width = if record.style & 8 != 0 {
            extent.width
        } else {
            parent_width
        };
        let x = if record.style & 8 != 0 { current.x } else { 0 };
        let y = if record.style & 8 != 0 || record.style & 3 == 2 {
            current.y
        } else if record.style & 3 == 3 {
            (self
                .windows()
                .get(parent)
                .map_err(E::from)
                .map_err(HostError::User32)?
                .description
                .extent
                .height as i32)
                .saturating_sub(height as i32)
        } else {
            0
        };
        if current.x == x && current.y == y && extent.width == width && extent.height == height {
            return Ok(User32Outcome32::Return(0));
        }
        self.begin_common_layout_position(
            [window.get(), 0, x as u32, y as u32, width, height, 0x14],
            ram,
            call,
        )
    }
}
