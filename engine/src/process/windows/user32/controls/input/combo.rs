use super::*;
use crate::windows::{
    user32::{Point32, Rect32, WindowData32},
    window::{WindowDescription32, WindowExtent32},
};
#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct Dropdown32 {
    pub window: WindowHandle32,
    pub popup: WindowHandle32,
    pub capture: Option<WindowHandle32>,
    pub thread: u32,
    pub events: [Option<WindowMessage32>; 6],
    pub index: usize,
    pub ready: bool,
    pub result: u32,
}
struct Popup32 {
    description: WindowDescription32,
    data: WindowData32,
}
impl User32Context32 {
    pub(super) fn combo_input(
        &mut self,
        window: WindowHandle32,
        args: &[u32],
        call: User32CallContext32,
        metrics: InputMetrics32,
    ) -> Result<Option<User32Outcome32>, E> {
        let data = self.state.record(window)?;
        let description = self.windows().get(window)?.description;
        let state = self.state.control_state(window)?;
        let simple = data.style & 3 == 1;
        let height = if metrics.height > 0 {
            metrics.height
        } else {
            state.item_height
        };
        if matches!(args[1], 1 | 0x30 | 0x47) && height > 0 {
            let mut data = data;
            let mut description = description;
            let collapsed = height.checked_add(4).ok_or(E::Win32(87))? as u32;
            let drop_height = if args[1] == 1 || description.extent.height != collapsed {
                description
                    .extent
                    .height
                    .saturating_sub(collapsed)
                    .max(height as u32)
            } else {
                state.drop_height
            };
            let bottom = data
                .outer
                .top
                .checked_add(collapsed as i32)
                .ok_or(E::Win32(87))?;
            if args[1] == 0x30 && args[3] != 0 {
                self.state.check_control_changes(1)?;
            }
            if !simple {
                data.outer.bottom = bottom;
                description.extent.height = collapsed;
                self.state
                    .windows_mut()
                    .update_description(window, description)?;
                self.state.update(data)?;
            }
            let state = self.state.control(window)?;
            state.item_height = height;
            state.drop_height = drop_height;
            if args[1] == 0x30 {
                state.font = args[2];
                if args[3] != 0 {
                    self.state
                        .invalidate_control(window)
                        .expect("combo font redraw preflighted");
                }
                return Ok(Some(User32Outcome32::Return(0)));
            }
            return Ok(None);
        }
        if args[1] == 0x157 {
            return Ok(Some(User32Outcome32::Return(u32::from(state.dropped))));
        }
        if args[1] == 0x215
            && (state.closing
                || args[3] == window.get()
                || state.popup.is_some_and(|popup| popup.get() == args[3]))
        {
            return Ok(Some(User32Outcome32::Return(0)));
        }
        if state.closing && matches!(args[1], 0x1f | 0x0a) {
            return Ok(Some(User32Outcome32::Return(0)));
        }
        let dropped = state.dropped;
        let mut selected = state.selected;
        let mut saved = state.drop_selection;
        let mut mouse = state.mouse_down;
        let focus = None;
        let mut capture = None;
        let mut codes = [0; 3];
        let mut count = 0;
        let mut accept = false;
        let mut cancel = false;
        let mut open = false;
        let (x, y) = point(args[3]);
        let top = height.checked_add(4).ok_or(E::Win32(87))?;
        let popup = state
            .popup
            .filter(|popup| self.windows().get(*popup).is_ok());
        let row = y - top - 1;
        let row = if height > 0 && x >= 1 && x < description.extent.width as i32 - 1 && row >= 0 {
            let index = state.scroll.2.max(0) as usize + (row / height) as usize;
            let drop_height = popup
                .and_then(|popup| self.windows().get(popup).ok())
                .map_or(state.drop_height, |record| record.description.extent.height);
            (row < (drop_height as i32 - 2) && index < state.items.len()).then_some(index)
        } else {
            None
        };
        match args[1] {
            0x14f => {
                if simple {
                    return Ok(Some(User32Outcome32::Return(1)));
                }
                if state.closing {
                    return Ok(Some(User32Outcome32::Return(1)));
                }
                if state.opening {
                    if args[2] == 0 {
                        self.state.control(window)?.opening = false;
                    }
                    return Ok(Some(User32Outcome32::Return(1)));
                }
                if args[2] != 0 && description.enabled {
                    open = !dropped;
                } else {
                    cancel = dropped;
                }
            }
            0x201 | 0x203 if description.enabled => {
                if simple || dropped {
                    if let Some(row) = row {
                        selected = Some(row);
                        mouse = true;
                        capture = Some(Some(popup.unwrap_or(window)));
                    } else if dropped {
                        cancel = true;
                    }
                } else {
                    open = true;
                }
            }
            0x200 if dropped && self.windows().capture() == Some(window) => {
                if let Some(row) = row {
                    selected = Some(row);
                }
            }
            0x202 if description.enabled && (simple || dropped) => {
                if let Some(row) = row {
                    selected = Some(row);
                    accept = true;
                }
                mouse = false;
            }
            0x100 | 0x104 if description.enabled => match args[2] {
                0x73 if args[1] == 0x100 => {
                    if dropped {
                        cancel = true
                    } else {
                        open = true
                    }
                }
                0x26 | 0x28 if args[1] == 0x104 => {
                    if dropped {
                        accept = true
                    } else {
                        open = true
                    }
                }
                13 if dropped => accept = true,
                27 if dropped => cancel = true,
                0x21..=0x28 if args[1] == 0x100 => {
                    selected = lists::key_selection(selected, state.items.len(), args[2], 8)
                }
                _ => return Ok(None),
            },
            7 | 8 => {
                codes[count] = if args[1] == 7 { 3 } else { 4 };
                count += 1;
                if args[1] == 8
                    && dropped
                    && !state.closing
                    && args[2] != popup.map_or(0, WindowHandle32::get)
                {
                    cancel = true;
                }
            }
            0x0a if args[2] != 0 => return Ok(None),
            0x1f | 0x215 | 0x0a => {
                if dropped {
                    cancel = true;
                }
            }
            _ => return Ok(None),
        }
        if height <= 0 && (open || row.is_some()) {
            return Err(E::Win32(50));
        }
        let prepared = if open {
            Some(self.prepare_combo_popup(window, height, call.thread_id, popup)?)
        } else {
            None
        };
        if open {
            saved = selected;
            codes[count] = 7;
            count += 1;
        }
        let mut after_codes = [0; 3];
        let mut after_count = 0;
        let closing = accept || cancel;
        if accept {
            codes[count] = 9;
            count += 1;
            if !simple {
                after_codes[after_count] = 8;
                after_count += 1;
            }
            if selected != saved {
                after_codes[after_count] = 1;
                after_count += 1;
            }
        } else if cancel {
            selected = saved;
            mouse = false;
            codes[count] = 10;
            count += 1;
            after_codes[after_count] = 8;
            after_count += 1;
        } else if selected != state.selected && args[1] == 0x100 {
            codes[count] = 1;
            count += 1;
        }
        if closing
            && (self.windows().capture() == Some(window)
                || popup.is_some_and(|popup| self.windows().capture() == Some(popup)))
        {
            capture = Some(None);
        }
        let changes = 1
            + u64::from(popup.is_some() || prepared.is_some())
            + u64::from(prepared.is_some() && popup.is_none());
        self.check_control_transition(
            window,
            call.thread_id,
            focus,
            capture,
            &codes[..count],
            changes,
        )?;
        let popup = if let Some(prepared) = prepared {
            if let Some(popup) = popup {
                self.state
                    .windows_mut()
                    .update_description(popup, prepared.description)
                    .expect("cached popup layout checked");
                self.state
                    .update(prepared.data)
                    .expect("cached popup remains owned");
                Some(popup)
            } else {
                let handle =
                    self.state
                        .publish_window(prepared.description, prepared.data, Box::default());
                self.state.set_control_list_owner(handle, window);
                Some(handle)
            }
        } else {
            popup
        };
        let events =
            self.commit_control_transition(window, call.thread_id, focus, capture, &codes[..count]);
        if let Some(popup) = popup {
            let mut description = self
                .windows()
                .get(popup)
                .expect("popup checked")
                .description;
            description.visible = dropped && !open;
            self.state
                .windows_mut()
                .update_description(popup, description)
                .expect("popup extent checked");
            self.state
                .invalidate_control(popup)
                .expect("popup redraw preflighted");
        }
        let state = self.state.control(window).expect("combo checked");
        state.popup = popup;
        state.dropped = dropped;
        state.opening = open;
        state.closing = closing;
        state.selected = selected;
        state.drop_selection = saved;
        state.mouse_down = mouse;
        if height > 0 {
            state.item_height = height;
        }
        let page = ((state.drop_height as i32 - 2) / height.max(1)).max(1) as usize;
        lists::reveal_selection(state, page);
        self.state
            .invalidate_control(window)
            .expect("combo redraw preflighted");
        let result = u32::from(args[1] == 0x14f);
        if open {
            let popup = popup.expect("prepared canonical combo popup");
            return self
                .control_start(
                    super::super::super::callback::Continuation32::ControlDropdown(Dropdown32 {
                        window,
                        popup,
                        thread: call.thread_id,
                        capture: if args[1] == 0x14f { None } else { Some(popup) },
                        events,
                        index: 0,
                        ready: false,
                        result,
                    }),
                )
                .map(Some);
        }
        if closing {
            let after = self.commit_control_transition(
                window,
                call.thread_id,
                None,
                None,
                &after_codes[..after_count],
            );
            return self
                .control_start(super::super::super::callback::Continuation32::ControlClose(
                    super::Close32 {
                        window,
                        popup,
                        events,
                        after,
                        index: 0,
                        ready: false,
                        result,
                    },
                ))
                .map(Some);
        }
        self.control_sequence(events, result).map(Some)
    }
    pub(in crate::process::windows::user32) fn complete_control_dropdown(
        &mut self,
        mut drop: Dropdown32,
    ) -> Result<User32Outcome32, E> {
        if !drop.ready && drop.events.get(drop.index + 1).copied().flatten().is_some() {
            drop.index += 1;
            self.pending.last_mut().unwrap().continuation =
                super::super::super::callback::Continuation32::ControlDropdown(drop);
            return self.next_outcome();
        }
        drop.ready = true;
        self.pending.last_mut().unwrap().continuation =
            super::super::super::callback::Continuation32::ControlDropdown(drop);
        let show = self
            .state
            .control_state(drop.window)
            .is_ok_and(|state| state.opening && state.popup == Some(drop.popup))
            && self
                .windows()
                .get(drop.window)
                .is_ok_and(|record| record.description.enabled)
            && self.windows().get(drop.popup).is_ok();
        if !show {
            if let Ok(state) = self.state.control(drop.window) {
                if state.popup == Some(drop.popup) {
                    state.opening = false;
                }
            }
            self.pending.pop();
            return Ok(User32Outcome32::Return(drop.result));
        }
        self.windows()
            .check_focus_capture(drop.thread, None, drop.capture.map(Some))?;
        self.state.check_control_changes(2)?;
        let events = self.commit_control_transition(
            drop.window,
            drop.thread,
            None,
            drop.capture.map(Some),
            &[],
        );
        let mut description = self.windows().get(drop.popup)?.description;
        description.visible = true;
        self.state
            .windows_mut()
            .update_description(drop.popup, description)
            .expect("popup checked");
        let state = self.state.control(drop.window).expect("combo checked");
        state.opening = false;
        state.dropped = true;
        self.state
            .invalidate_control(drop.popup)
            .expect("popup invalidation preflighted");
        self.state
            .invalidate_control(drop.window)
            .expect("combo invalidation preflighted");
        self.pending.last_mut().unwrap().continuation =
            super::super::super::callback::Continuation32::Sequence {
                events,
                index: 0,
                result: drop.result,
            };
        self.next_outcome()
    }
    fn prepare_combo_popup(
        &self,
        window: WindowHandle32,
        height: i32,
        thread: u32,
        existing: Option<WindowHandle32>,
    ) -> Result<Popup32, E> {
        let owner = self.state.record(window)?;
        let state = self.state.control_state(window)?;
        let extent = self.windows().get(window)?.description.extent;
        let height = state
            .drop_height
            .max(height as u32 + 2)
            .min((self.input.desktop.bottom - self.input.desktop.top).max(1) as u32);
        let mut top = owner
            .client_origin
            .y
            .checked_add(extent.height as i32)
            .ok_or(E::Win32(87))?;
        if i64::from(top) + i64::from(height) > i64::from(self.input.desktop.bottom) {
            top = owner
                .client_origin
                .y
                .checked_sub(height as i32)
                .ok_or(E::Win32(87))?
                .max(self.input.desktop.top);
        }
        let bottom = top.checked_add(height as i32).ok_or(E::Win32(87))?;
        let left = owner
            .client_origin
            .x
            .min(self.input.desktop.right.saturating_sub(extent.width as i32))
            .max(self.input.desktop.left);
        let right = left.checked_add(extent.width as i32).ok_or(E::Win32(87))?;
        let description = WindowDescription32 {
            thread_id: thread,
            procedure: self.builtin_procedures.ok_or(E::Win32(1407))?[usize::from(owner.unicode)],
            extent: WindowExtent32 {
                width: extent.width,
                height,
            },
            visible: false,
            enabled: true,
        };
        let handle = if let Some(existing) = existing {
            let old = self.windows().get(existing)?;
            if old.description.thread_id != thread
                || self.state.control_list_owner(existing)? != Some(window)
            {
                return Err(E::InvalidRequest);
            }
            existing
        } else {
            self.state.check_create(description)?
        };
        Ok(Popup32 {
            description,
            data: WindowData32 {
                handle,
                class_atom: self.state.standard_atom(ControlKind32::ListBox),
                instance: owner.instance,
                style: 0x8080_0001,
                ex_style: 0x80,
                user_data: 0,
                parent: Some(window),
                menu: 0,
                outer: Rect32 {
                    left,
                    top,
                    right,
                    bottom,
                },
                client_origin: Point32 { x: left, y: top },
                unicode: owner.unicode,
                destroying: false,
            },
        })
    }
}
