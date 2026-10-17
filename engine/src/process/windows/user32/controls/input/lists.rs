use super::*;
impl User32Context32 {
    pub(super) fn list_input(
        &mut self,
        window: WindowHandle32,
        args: &[u32],
        call: User32CallContext32,
        metrics: InputMetrics32,
    ) -> Result<Option<User32Outcome32>, E> {
        if let Some(owner) = self.state.control_list_owner(window)? {
            let from = self.state.record(window)?.client_origin;
            let to = self.state.record(owner)?.client_origin;
            let (x, y) = point(args[3]);
            let mut forwarded = [
                owner.get(),
                args[1],
                args[2],
                ((x.saturating_add(from.x).saturating_sub(to.x) as u32) & 0xffff)
                    | ((y.saturating_add(from.y).saturating_sub(to.y) as u32) << 16),
            ];
            if !matches!(args[1], 0x200..=0x203) {
                forwarded[3] = args[3];
            }
            return self.control_input_message(
                owner,
                ControlKind32::ComboBox,
                &forwarded,
                call,
                metrics,
            );
        }
        let data = self.state.record(window)?;
        let description = self.windows().get(window)?.description;
        let state = self.state.control_state(window)?;
        let height = if metrics.height > 0 {
            metrics.height
        } else {
            state.item_height
        };
        if height <= 0 && matches!(args[1], 0x201..=0x203 | 0x100) {
            return Err(E::Win32(50));
        }
        let mut selected = state.selected;
        let mut mouse = state.mouse_down;
        let focus = None;
        let mut capture = None;
        let mut codes = [0; 1];
        let mut notify = false;
        let (x, y) = point(args[3]);
        let row = if x >= 1
            && x < (description.extent.width as i32 - 1)
            && y >= 1
            && y < (description.extent.height as i32 - 1)
            && height > 0
        {
            let index = state.scroll.2.max(0) as usize + ((y - 1) / height) as usize;
            (index < state.items.len()).then_some(index)
        } else {
            None
        };
        match args[1] {
            0x201 | 0x203 if description.enabled => {
                if data.style & (8 | 0x800) != 0 {
                    return Err(E::Win32(50));
                }

                capture = Some(Some(window));
                mouse = true;
                if let Some(row) = row {
                    selected = Some(row);
                }
                if args[1] == 0x203 {
                    codes[0] = 2;
                    notify = data.style & 1 != 0;
                }
            }
            0x200 if mouse && self.windows().capture() == Some(window) => {
                if let Some(row) = row {
                    selected = Some(row);
                }
            }
            0x202 if mouse => {
                mouse = false;
                if self.windows().capture() == Some(window) {
                    capture = Some(None);
                }
                notify = data.style & 1 != 0 && selected.is_some();
                codes[0] = 1;
            }
            0x100 if description.enabled && matches!(args[2], 0x21..=0x28) => {
                if data.style & (8 | 0x800) != 0 {
                    return Err(E::Win32(50));
                }
                selected = key_selection(
                    state.selected,
                    state.items.len(),
                    args[2],
                    ((description.extent.height as i32 - 2) / height.max(1)).max(1) as usize,
                );
                codes[0] = 1;
                notify = selected != state.selected && data.style & 1 != 0;
            }
            7 | 8 => {
                codes[0] = if args[1] == 7 { 4 } else { 5 };
                notify = data.style & 1 != 0;
                if args[1] == 8 {
                    mouse = false;
                    if self.windows().capture() == Some(window) {
                        capture = Some(None);
                    }
                }
            }
            0x1f | 0x215 => {
                mouse = false;
                if args[1] != 0x215 && self.windows().capture() == Some(window) {
                    capture = Some(None);
                }
            }
            _ => return Ok(None),
        }
        let codes = if notify { &codes[..] } else { &[] };
        self.check_control_transition(window, call.thread_id, focus, capture, codes, 1)?;
        let events = self.commit_control_transition(window, call.thread_id, focus, capture, codes);
        let state = self.state.control(window).expect("list checked");
        state.selected = selected;
        state.mouse_down = mouse;
        if height > 0 {
            state.item_height = height;
        }
        if args[1] == 0x100 {
            reveal_selection(
                state,
                ((description.extent.height as i32 - 2) / height.max(1)).max(1) as usize,
            );
        }
        self.state
            .invalidate_control(window)
            .expect("list invalidation preflighted");
        self.control_sequence(events, 0).map(Some)
    }
}
pub(super) fn key_selection(
    selected: Option<usize>,
    count: usize,
    key: u32,
    page: usize,
) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let current = selected.unwrap_or(0);
    Some(match key {
        0x24 => 0,
        0x23 => count - 1,
        0x26 | 0x25 => current.saturating_sub(1),
        0x28 | 0x27 => {
            if selected.is_none() {
                0
            } else {
                (current + 1).min(count - 1)
            }
        }
        0x21 => current.saturating_sub(page),
        0x22 => current.saturating_add(page).min(count - 1),
        _ => current,
    })
}
pub(super) fn reveal_selection(state: &mut crate::windows::controls::ControlState32, page: usize) {
    if let Some(selected) = state.selected {
        let first = state.scroll.2.max(0) as usize;
        if selected < first {
            state.scroll.2 = selected as i32;
        } else if selected >= first + page {
            state.scroll.2 = selected.saturating_sub(page - 1) as i32;
        }
    }
}
