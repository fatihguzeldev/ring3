use super::*;
impl User32Context32 {
    pub(super) fn button_input(
        &mut self,
        window: WindowHandle32,
        args: &[u32],
        call: User32CallContext32,
    ) -> Result<Option<User32Outcome32>, E> {
        let data = self.state.record(window)?;
        let description = self.windows().get(window)?.description;
        let style = data.style & 15;
        let state = self.state.control_state(window)?;
        let (x, y) = point(args[3]);
        let inside = x >= 0
            && y >= 0
            && x < description.extent.width as i32
            && y < description.extent.height as i32;
        let mut mouse = state.mouse_down;
        let mut key = state.key_down;
        let mut pressed = state.pressed;
        let focus = None;
        let mut capture = None;
        let mut activate = false;
        match args[1] {
            0x201 | 0x203 if style != 7 => {
                mouse = true;
                key = false;
                pressed = inside;

                capture = Some(Some(window));
            }
            0x200 if mouse && self.windows().capture() == Some(window) => pressed = inside,
            0x202 => {
                activate = mouse && pressed && inside && self.windows().capture() == Some(window);
                mouse = false;
                pressed = false;
                if self.windows().capture() == Some(window) {
                    capture = Some(None);
                }
            }
            0x100 if args[2] == 32 => {
                capture = Some(Some(window));
                key = true;
                pressed = true;
            }
            0x101 | 0x105 if args[2] != 9 => {
                activate = args[2] == 32 && key && pressed;
                key = false;
                mouse = false;
                pressed = false;
                if self.windows().capture() == Some(window) {
                    capture = Some(None);
                }
            }
            0x0a if args[2] != 0 => return Ok(None),
            0x1f | 0x215 | 8 | 0x0a => {
                mouse = false;
                key = false;
                pressed = false;
                if args[1] != 0x215 && self.windows().capture() == Some(window) {
                    capture = Some(None);
                }
            }
            7 => {}
            0xf5 if description.enabled && style != 7 => activate = true,
            0xf5 => return Ok(Some(User32Outcome32::Return(0))),
            _ => return Ok(None),
        }
        let radios = if activate && style == 9 {
            self.state.control_radio_group(window)?
        } else {
            Vec::new()
        };
        let codes: &[u32] = if activate { &[0] } else { &[] };
        self.check_control_transition(
            window,
            call.thread_id,
            focus,
            capture,
            codes,
            radios.len() as u64 + 1,
        )?;
        let events = self.commit_control_transition(window, call.thread_id, focus, capture, codes);
        for radio in radios {
            self.state
                .control(radio)
                .expect("radio group checked")
                .check = u32::from(radio == window);
            self.state
                .invalidate_control(radio)
                .expect("radio invalidation preflighted");
        }
        let state = self.state.control(window).expect("button checked");
        state.mouse_down = mouse;
        state.key_down = key;
        state.pressed = pressed;
        if activate {
            match style {
                3 => state.check = 1 - state.check.min(1),
                6 => state.check = (state.check + 1) % 3,
                9 => state.check = 1,
                _ => {}
            }
        }
        self.state
            .invalidate_control(window)
            .expect("button invalidation preflighted");
        self.control_sequence(events, 0).map(Some)
    }
}
