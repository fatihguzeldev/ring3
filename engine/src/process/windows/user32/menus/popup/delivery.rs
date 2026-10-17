use super::types::{After32, Stage32, Window32};
use crate::{
    memory::AddressSpace,
    process::windows::{
        gdi::GdiRuntime32,
        user32::{User32Context32, User32Outcome32, callback::Continuation32, memory},
    },
    windows::{
        user32::{
            Message32, Point32, User32Api32 as Api, User32Error32 as E,
            menus::{MenuAction32, MenuInput32},
        },
        window::WindowHandle32,
    },
};

impl User32Context32 {
    pub(super) fn popup_index(&self, token: u64) -> Result<usize, E> {
        self.menu_popups
            .plans
            .iter()
            .position(|plan| plan.token == token)
            .ok_or(E::InvalidToken)
    }

    pub(super) fn popup_scratch(
        &self,
        token: u64,
    ) -> Result<super::super::super::memory::Scratch32, E> {
        let pending = self
            .pending
            .iter()
            .find(|pending| pending.token == token)
            .ok_or(E::InvalidToken)?;
        if let Continuation32::MenuPopup { scratch, .. } = pending.continuation {
            Ok(scratch)
        } else {
            Err(E::InvalidToken)
        }
    }

    pub(in crate::process::windows::user32) fn pending_popup_message(
        &self,
        token: u64,
    ) -> Result<Option<[u32; 4]>, E> {
        let Some(pending) = self.pending.last().filter(|pending| pending.token == token) else {
            return Err(E::InvalidToken);
        };
        if !matches!(pending.continuation, Continuation32::MenuPopup { .. }) {
            return Ok(None);
        }
        let plan = &self.menu_popups.plans[self.popup_index(token)?];
        if plan.child.is_some() {
            return Err(E::InvalidToken);
        }
        let notify = plan.flags & 0x80 == 0;
        let event = match plan.stage {
            Stage32::Enter if notify => [0x211, 1, 0],
            Stage32::Initialize if notify => [0x116, plan.root.get(), 0],
            Stage32::MeasureCall(_, _) => [0x2c, 0, self.popup_scratch(token)?.address.0 + 64],
            Stage32::DrawCall(_, _) => [0x2b, 0, self.popup_scratch(token)?.address.0 + 64],
            Stage32::InitializePopup(depth) if notify => {
                let popup = &plan.windows[depth];
                [0x117, popup.menu.get(), popup.position as u32]
            }
            Stage32::Uninitialize(depth) if notify => [0x125, plan.windows[depth].menu.get(), 0],
            Stage32::Exit if notify => [0x212, 1, 0],
            Stage32::FinalSelect => [0x11f, 0xffff0000, 0],
            Stage32::Select => {
                let (depth, selected) = plan.selected.ok_or(E::InvalidRequest)?;
                let menu = plan.windows[depth].menu;
                if let Some(position) = selected {
                    let item = self.menus.item(menu, position as u32, true)?;
                    let flags = 0x80
                        | item.state
                        | item.kind
                        | if item.submenu.is_some() { 0x10 } else { 0 };
                    [
                        0x11f,
                        ((flags & 0xffff) << 16)
                            | if item.submenu.is_some() {
                                position as u32
                            } else {
                                item.id & 0xffff
                            },
                        menu.get(),
                    ]
                } else {
                    [0x11f, 0xffff0000, 0]
                }
            }
            Stage32::Character => {
                let depth = plan.tracker.depth() - 1;
                [
                    0x120,
                    u32::from(plan.character.ok_or(E::InvalidRequest)?) | (0x10 << 16),
                    plan.windows[depth].menu.get(),
                ]
            }
            _ => return Ok(None),
        };
        if self.windows().get(plan.owner).is_err() {
            return Ok(None);
        }
        Ok(Some([plan.owner.get(), event[0], event[1], event[2]]))
    }

    pub(in crate::process::windows::user32) fn resume_popup_menu(
        &mut self,
        token: u64,
        ram: &mut AddressSpace,
        mut gdi: Option<&mut GdiRuntime32>,
    ) -> Result<User32Outcome32, E> {
        let admitted = self.menu_popups.plans[self.popup_index(token)?].stage;
        for _ in 0..64 {
            self.popup_scratch(token)?.check(ram)?;
            let index = self.popup_index(token)?;
            let plan = &self.menu_popups.plans[index];
            if plan.child.is_some() {
                return Err(E::InvalidToken);
            }
            let stage = plan.stage;
            if (plan.cancelled
                || self.windows().get(plan.owner).is_err()
                || plan
                    .windows
                    .iter()
                    .any(|window| !self.menus.contains(window.menu)))
                && !matches!(
                    stage,
                    Stage32::Uninitialize(_)
                        | Stage32::DrawFinish(_, _)
                        | Stage32::Destroy(_)
                        | Stage32::Release
                        | Stage32::FinalSelect
                        | Stage32::Exit
                        | Stage32::Finish
                )
            {
                self.menu_popups.plans[index].cancelled = true;
                self.menu_popups.plans[index].command = None;
                self.popup_after(token, After32::Finish)?;
                continue;
            }
            if let Some(args) = self.pending_popup_message(token)? {
                let call = plan.call;
                let outcome = self.send_message(&args, call)?;
                return self.popup_delegate(token, outcome);
            }
            let call = plan.call;
            let scratch = self.popup_scratch(token)?;
            let outcome = match stage {
                Stage32::Enter => {
                    self.popup_completed(token, 0)?;
                    continue;
                }
                Stage32::Initialize => {
                    self.popup_completed(token, 0)?;
                    continue;
                }
                Stage32::InitializePopup(depth) => {
                    self.menu_popups.plans[index].stage = Stage32::Layout(depth);
                    continue;
                }
                Stage32::Layout(depth) => {
                    self.layout_popup(token, depth, ram, gdi.as_deref_mut().ok_or(E::Win32(50))?)?;
                    continue;
                }
                Stage32::MeasurePrepare(depth, position) => {
                    self.prepare_menu_measure(
                        token,
                        depth,
                        position,
                        ram,
                        gdi.as_deref().ok_or(E::Win32(50))?,
                    )?;
                    return Ok(User32Outcome32::Wait {
                        continuation: token,
                    });
                }
                Stage32::MeasureRead(depth, position) => {
                    self.accept_menu_measure(token, depth, position, ram)?;
                    continue;
                }
                Stage32::DrawPrepare(depth, position) => {
                    self.prepare_menu_draw(
                        token,
                        depth,
                        position,
                        ram,
                        gdi.as_deref_mut().ok_or(E::Win32(50))?,
                    )?;
                    return Ok(User32Outcome32::Wait {
                        continuation: token,
                    });
                }
                Stage32::DrawFinish(depth, position) => {
                    self.finish_menu_draw(
                        token,
                        depth,
                        position,
                        gdi.as_deref_mut().ok_or(E::Win32(50))?,
                    )?;
                    continue;
                }
                Stage32::MeasureCall(_, _) | Stage32::DrawCall(_, _) => {
                    return Err(E::InvalidToken);
                }
                Stage32::Create(depth) => {
                    let popup = &plan.windows[depth];
                    let visual = popup.visual.as_ref().ok_or(E::InvalidRequest)?;
                    self.create_window(
                        Api::CreateWindowExW,
                        &[
                            0x80,
                            scratch.address.0 + 32,
                            0,
                            0x80800000,
                            popup.origin.x as u32,
                            popup.origin.y as u32,
                            visual.layout.extent.x as u32,
                            visual.layout.extent.y as u32,
                            plan.owner.get(),
                            0,
                            call.instance,
                            0,
                        ],
                        ram,
                        call,
                    )?
                }
                Stage32::Position(depth) => {
                    let popup = &plan.windows[depth];
                    let visual = popup.visual.as_ref().ok_or(E::InvalidRequest)?;
                    self.move_window(
                        Api::SetWindowPos,
                        &[
                            popup.handle.ok_or(E::InvalidRequest)?.get(),
                            0,
                            popup.origin.x as u32,
                            popup.origin.y as u32,
                            visual.layout.extent.x as u32,
                            visual.layout.extent.y as u32,
                            0x14,
                        ],
                        ram,
                        call,
                    )?
                }
                Stage32::Show(depth) => self.show_window(
                    &[
                        plan.windows[depth].handle.ok_or(E::InvalidRequest)?.get(),
                        4,
                    ],
                    call,
                )?,
                Stage32::Capture => {
                    if admitted != stage {
                        return Ok(User32Outcome32::Wait {
                            continuation: token,
                        });
                    }
                    self.set_focus_capture(
                        Api::SetCapture,
                        &[plan.windows[0].handle.ok_or(E::InvalidRequest)?.get()],
                        call,
                    )?
                }
                Stage32::Paint(depth) => {
                    if plan.windows[depth]
                        .visual
                        .as_ref()
                        .is_some_and(|visual| visual.revision != self.menus.revision())
                    {
                        self.menu_popups.plans[index].stage = Stage32::Layout(depth);
                        continue;
                    }
                    self.paint_popup(token, depth, ram, gdi.as_deref_mut().ok_or(E::Win32(50))?)?;
                    self.menu_popups.plans[index].stage = Stage32::DrawPrepare(depth, 0);
                    continue;
                }
                Stage32::Wait => {
                    if !self.popup_ready(token) {
                        return Ok(User32Outcome32::Wait {
                            continuation: token,
                        });
                    }
                    self.menu_popups.plans[index].stage = Stage32::Retrieve;
                    continue;
                }
                Stage32::Retrieve => self.retrieve_message(
                    Api::GetMessageW,
                    &[scratch.address.0, 0, 0, 0, 0],
                    ram,
                    call,
                )?,
                Stage32::Dispatch => {
                    let message = memory::message(ram, scratch.address.0)?;
                    if message.message == 0x12 {
                        self.state.post_quit(message)?;
                    }
                    if let Some(input) = self.popup_input(token, message)? {
                        let (menus, plans) = (&self.menus, &mut self.menu_popups.plans);
                        let action = plans[index].tracker.input(menus, input)?;
                        self.popup_action(token, action)?;
                        continue;
                    }
                    self.translate_message(scratch.address.0, ram, call)?;
                    if (0x100..=0x109).contains(&message.message) {
                        self.menu_popups.plans[index].stage = Stage32::Wait;
                        return Ok(User32Outcome32::Wait {
                            continuation: token,
                        });
                    }
                    self.dispatch_message(scratch.address.0, ram, call)?
                }
                Stage32::Select => {
                    self.popup_completed(token, 0)?;
                    continue;
                }
                Stage32::Character => {
                    self.popup_completed(token, 0)?;
                    continue;
                }
                Stage32::Uninitialize(depth) => {
                    self.menu_popups.plans[index].stage = Stage32::Destroy(depth);
                    continue;
                }
                Stage32::Destroy(depth) => {
                    if admitted != stage {
                        return Ok(User32Outcome32::Wait {
                            continuation: token,
                        });
                    }
                    if let Some(window) = plan.windows[depth]
                        .handle
                        .filter(|&window| self.windows().get(window).is_ok())
                    {
                        self.begin_destroy(window, call.thread_id, 1, None, true, false)?
                    } else {
                        self.popup_completed(token, 0)?;
                        continue;
                    }
                }
                Stage32::Release => {
                    let target = plan
                        .restore_capture
                        .filter(|&window| self.windows().get(window).is_ok());
                    if self.windows().capture() == target || !plan.captured {
                        self.popup_completed(token, 0)?;
                        continue;
                    }
                    if admitted != stage {
                        return Ok(User32Outcome32::Wait {
                            continuation: token,
                        });
                    }
                    if let Some(target) = target {
                        self.set_focus_capture(Api::SetCapture, &[target.get()], call)?
                    } else {
                        self.set_focus_capture(Api::ReleaseCapture, &[], call)?
                    }
                }
                Stage32::FinalSelect => {
                    self.popup_completed(token, 0)?;
                    continue;
                }
                Stage32::Exit => {
                    self.popup_completed(token, 0)?;
                    continue;
                }
                Stage32::Finish => return self.finish_popup(token, ram),
            };
            return match outcome {
                User32Outcome32::Return(value) => {
                    self.popup_completed(token, value)?;
                    Ok(User32Outcome32::Wait {
                        continuation: token,
                    })
                }
                outcome => self.popup_delegate(token, outcome),
            };
        }
        Err(E::InvalidRequest)
    }

    fn popup_delegate(
        &mut self,
        token: u64,
        outcome: User32Outcome32,
    ) -> Result<User32Outcome32, E> {
        let child = match outcome {
            User32Outcome32::Callback(callback) => callback.continuation,
            User32Outcome32::Wait { continuation } => continuation,
            _ => return Err(E::InvalidRequest),
        };
        let index = self.popup_index(token)?;
        let plan = &mut self.menu_popups.plans[index];
        plan.child = Some(child);
        plan.completed = None;
        Ok(outcome)
    }

    pub(in crate::process::windows::user32) fn complete_popup_child(
        &mut self,
        child: u64,
        outcome: User32Outcome32,
    ) -> Result<User32Outcome32, E> {
        let User32Outcome32::Return(value) = outcome else {
            return Ok(outcome);
        };
        let Some(pending) = self.pending.last() else {
            return Ok(outcome);
        };
        let Continuation32::MenuPopup { plan: token, .. } = pending.continuation else {
            return Ok(outcome);
        };
        let index = self.popup_index(token)?;
        if self.menu_popups.plans[index].child != Some(child) {
            return Ok(outcome);
        }
        self.menu_popups.plans[index].child = None;
        self.menu_popups.plans[index].completed = Some(child);
        self.popup_completed(token, value)?;
        Ok(User32Outcome32::Wait {
            continuation: token,
        })
    }

    fn popup_completed(&mut self, token: u64, result: u32) -> Result<(), E> {
        let index = self.popup_index(token)?;
        let plan = &mut self.menu_popups.plans[index];
        plan.stage = match plan.stage {
            Stage32::Enter => Stage32::Initialize,
            Stage32::Initialize => Stage32::InitializePopup(0),
            Stage32::InitializePopup(depth) => Stage32::Layout(depth),
            Stage32::Create(depth) => {
                let window = WindowHandle32::try_from(result)
                    .ok()
                    .filter(|&window| self.state.windows().get(window).is_ok());
                plan.windows[depth].handle = window;
                if window.is_none() {
                    plan.cancelled = true;
                    Stage32::Release
                } else {
                    Stage32::Show(depth)
                }
            }
            Stage32::Position(depth) => Stage32::Paint(depth),
            Stage32::MeasureCall(depth, position) => Stage32::MeasureRead(depth, position),
            Stage32::DrawCall(depth, position) => Stage32::DrawFinish(depth, position),
            Stage32::Show(depth) => {
                if depth == 0 && !plan.captured {
                    Stage32::Capture
                } else {
                    Stage32::Paint(depth)
                }
            }
            Stage32::Capture => {
                plan.captured = true;
                Stage32::Paint(0)
            }
            Stage32::Retrieve => Stage32::Dispatch,
            Stage32::Dispatch => Stage32::Wait,
            Stage32::Select => {
                let after = plan.after;
                if matches!(after, After32::Wait) {
                    plan.stage = if plan.windows.iter().any(|window| {
                        window
                            .visual
                            .as_ref()
                            .is_some_and(|visual| visual.revision != self.menus.revision())
                    }) {
                        plan.draw_action = 1;
                        plan.draw_only = None;
                        Stage32::Layout(plan.windows.len() - 1)
                    } else {
                        Stage32::Wait
                    };
                } else {
                    self.popup_after(token, after)?;
                }
                return Ok(());
            }
            Stage32::Character => {
                let action = result >> 16;
                if action == 1 {
                    self.popup_after(token, After32::Finish)?;
                    return Ok(());
                }
                if matches!(action, 2 | 3) {
                    let depth = plan.tracker.depth() - 1;
                    let event = plan.tracker.input(
                        &self.menus,
                        MenuInput32::Point {
                            depth,
                            position: Some((result & 0xffff) as usize),
                            choose: action == 2,
                        },
                    )?;
                    self.popup_action(token, event)?;
                    return Ok(());
                }
                Stage32::Wait
            }
            Stage32::Uninitialize(depth) => Stage32::Destroy(depth),
            Stage32::Destroy(_) => {
                plan.windows.pop();
                let after = plan.after;
                self.popup_after(token, after)?;
                return Ok(());
            }
            Stage32::Release => {
                plan.captured = false;
                Stage32::FinalSelect
            }
            Stage32::FinalSelect => Stage32::Exit,
            Stage32::Exit => Stage32::Finish,
            _ => return Err(E::InvalidRequest),
        };
        Ok(())
    }

    pub(super) fn popup_after_drawing_exit(&mut self, token: u64) -> Result<(), E> {
        self.popup_after(token, After32::Finish)
    }

    fn popup_after(&mut self, token: u64, after: After32) -> Result<(), E> {
        let index = self.popup_index(token)?;
        let plan = &mut self.menu_popups.plans[index];
        plan.after = after;
        let retain = match after {
            After32::Finish => 0,
            After32::Open(depth) | After32::Close(depth) => depth,
            After32::Wait => plan.tracker.depth(),
        };
        if plan.windows.len() > retain {
            plan.stage = Stage32::Uninitialize(plan.windows.len() - 1);
        } else if let After32::Open(depth) = after {
            let level = plan.tracker.level(depth).ok_or(E::InvalidRequest)?;
            let position = plan
                .tracker
                .level(depth - 1)
                .and_then(|level| level.selected)
                .ok_or(E::InvalidRequest)?;
            plan.windows.push(Window32 {
                handle: None,
                menu: level.menu,
                position,
                origin: Point32::default(),
                visual: None,
                measurements: Vec::new(),
            });
            plan.stage = Stage32::InitializePopup(depth);
        } else if matches!(after, After32::Finish) {
            plan.stage = Stage32::Release;
        } else {
            plan.stage = Stage32::Layout(plan.windows.len() - 1);
        }
        Ok(())
    }

    fn popup_action(&mut self, token: u64, action: MenuAction32) -> Result<(), E> {
        let index = self.popup_index(token)?;
        let plan = &mut self.menu_popups.plans[index];
        match action {
            MenuAction32::None => plan.stage = Stage32::Wait,
            MenuAction32::Select { depth, position } => {
                let previous = plan
                    .selected
                    .filter(|&(previous_depth, _)| previous_depth == depth)
                    .and_then(|(_, position)| position);
                plan.selected = Some((depth, position));
                plan.draw_action = 2;
                plan.draw_only = Some([previous, position]);
                plan.paint_notify = true;
                plan.after = After32::Wait;
                self.popup_after(token, After32::Wait)?;
            }
            MenuAction32::Open {
                depth,
                menu,
                parent_position,
            } => {
                if plan
                    .tracker
                    .level(depth)
                    .is_none_or(|level| level.menu != menu)
                    || plan
                        .tracker
                        .level(depth - 1)
                        .and_then(|level| level.selected)
                        != Some(parent_position)
                {
                    return Err(E::InvalidRequest);
                }
                let selected = Some((
                    depth - 1,
                    plan.tracker
                        .level(depth - 1)
                        .and_then(|level| level.selected),
                ));
                if plan.selected != selected {
                    plan.selected = selected;
                    plan.after = After32::Open(depth);
                    plan.paint_notify = true;
                    plan.draw_action = 2;
                    plan.draw_only = Some([None, selected.and_then(|(_, position)| position)]);
                    plan.stage = Stage32::Layout(depth - 1);
                } else {
                    self.popup_after(token, After32::Open(depth))?;
                }
            }
            MenuAction32::Close { depth } => {
                self.popup_after(token, After32::Close(depth))?;
            }
            MenuAction32::Command { menu, position, id } => {
                plan.command = Some((menu, position, id));
                plan.exit_draw = true;
                plan.draw_action = 2;
                plan.draw_only = Some([Some(position), None]);
                plan.stage = Stage32::DrawPrepare(plan.tracker.depth() - 1, position);
            }
            MenuAction32::Cancel => {
                plan.cancelled = true;
                self.popup_after(token, After32::Finish)?;
            }
            MenuAction32::Unmatched(unit) => {
                plan.character = Some(unit);
                plan.stage = Stage32::Character;
            }
        }
        Ok(())
    }

    fn popup_input(&self, token: u64, message: Message32) -> Result<Option<MenuInput32>, E> {
        let plan = &self.menu_popups.plans[self.popup_index(token)?];
        Ok(match message.message {
            0x1f | 0x12 => Some(MenuInput32::Cancel),
            0x100 | 0x104 => match message.wparam {
                0x26 => Some(MenuInput32::Next(true)),
                0x28 => Some(MenuInput32::Next(false)),
                0x24 => Some(MenuInput32::Home),
                0x23 => Some(MenuInput32::End),
                0x25 => Some(MenuInput32::Previous),
                0x1b => Some(MenuInput32::Back),
                0x27 => Some(MenuInput32::Forward),
                0xd => Some(MenuInput32::Open),
                0x12 => Some(MenuInput32::Cancel),
                _ => None,
            },
            0x102 | 0x106 => Some(MenuInput32::Mnemonic(message.wparam as u16)),
            0x200 | 0x201 | 0x202 | 0x204 | 0x205 => {
                let click = matches!(message.message, 0x202 | 0x205);
                if matches!(message.message, 0x204 | 0x205) && plan.flags & 2 == 0 {
                    return Ok(None);
                }
                let found = plan
                    .windows
                    .iter()
                    .enumerate()
                    .rev()
                    .find_map(|(depth, window)| {
                        let visual = window.visual.as_ref()?;
                        let point = Point32 {
                            x: message.point.x.checked_sub(window.origin.x)?,
                            y: message.point.y.checked_sub(window.origin.y)?,
                        };
                        visual
                            .layout
                            .item_at(point)
                            .map(|position| (depth, position))
                    });
                match found {
                    Some((depth, position)) => Some(MenuInput32::Point {
                        depth,
                        position: Some(position),
                        choose: click,
                    }),
                    None if matches!(message.message, 0x201 | 0x204) => Some(MenuInput32::Cancel),
                    None => None,
                }
            }
            _ => None,
        })
    }

    fn finish_popup(&mut self, token: u64, ram: &mut AddressSpace) -> Result<User32Outcome32, E> {
        let index = self.popup_index(token)?;
        let plan = &self.menu_popups.plans[index];
        let command = plan
            .command
            .filter(|_| !plan.cancelled && self.windows().get(plan.owner).is_ok());
        if command.is_some() && plan.flags & 0x100 == 0 {
            self.state.can_post(1)?;
        }
        self.popup_scratch(token)?.release(ram)?;
        let plan = self.menu_popups.plans.remove(index);
        self.pending.pop();
        if let Some((_, _, id)) = command
            && plan.flags & 0x100 == 0
        {
            self.state
                .post(Message32 {
                    thread_id: plan.call.thread_id,
                    window: plan.owner.get(),
                    message: 0x111,
                    wparam: id & 0xffff,
                    lparam: 0,
                    time_ms: self.timers.tick,
                    point: self.runtime_input.cursor,
                })
                .expect("popup command queue slot was preflighted");
        }
        Ok(User32Outcome32::Return(if plan.flags & 0x100 != 0 {
            command.map_or(0, |(_, _, id)| id)
        } else {
            1
        }))
    }

    pub(in crate::process::windows::user32) fn popup_ready(&self, token: u64) -> bool {
        self.popup_index(token).is_ok_and(|index| {
            let plan = &self.menu_popups.plans[index];
            plan.stage != Stage32::Wait
                || plan.cancelled
                || self.windows().get(plan.owner).is_err()
                || self
                    .state
                    .find_message(plan.call.thread_id, 0, 0, 0)
                    .is_ok_and(|message| message.is_some())
                || self.timer_message_ready(plan.call.thread_id, 0, 0, 0)
                || self
                    .state
                    .paint_message(
                        plan.call.thread_id,
                        0,
                        0,
                        0,
                        u32::MAX,
                        self.timers.tick,
                        self.runtime_input.cursor,
                    )
                    .is_ok_and(|message| message.is_some())
        })
    }
}
