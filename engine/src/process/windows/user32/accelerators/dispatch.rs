use super::super::{
    User32CallContext32, User32Context32, User32Outcome32, callback::Continuation32, memory,
};
use crate::{
    memory::AddressSpace,
    windows::{
        user32::{
            User32Api32 as Api, User32Error32 as E, accelerators::AcceleratorHandle32,
            menus::MenuHandle32,
        },
        window::WindowHandle32,
    },
};

#[derive(Clone, Copy)]
enum Stage32 {
    Initialize,
    Popup(usize),
    Command,
    Finish,
}

#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct Dispatch32 {
    window: WindowHandle32,
    thread: u32,
    command: u16,
    menu: Option<MenuHandle32>,
    stage: Stage32,
    child: Option<u64>,
    completed: Option<u64>,
}

#[derive(Clone, Copy)]
struct Popup32 {
    menu: MenuHandle32,
    position: u32,
}

impl User32Context32 {
    pub(in crate::process::windows::user32) fn translate_accelerator(
        &mut self,
        api: Api,
        args: &[u32],
        ram: &AddressSpace,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, E> {
        let window = WindowHandle32::try_from(args[0])?;
        self.windows().get(window)?;
        let table = self
            .accelerators
            .entries(AcceleratorHandle32::try_from(args[1])?)?;
        let mut message = memory::message(ram, args[2])?;
        if !api.wide() && matches!(message.message, 0x102 | 0x106) {
            let Ok(key) = u16::try_from(message.wparam) else {
                return Ok(User32Outcome32::Return(0));
            };
            let bytes = key.to_le_bytes();
            let units = call
                .ansi
                .decode(&bytes[..if bytes[1] == 0 { 1 } else { 2 }], false)
                .map_err(|_| E::Win32(1113))?;
            let Some(&unit) = units.first().filter(|_| units.len() == 1) else {
                return Ok(User32Outcome32::Return(0));
            };
            message.wparam = u32::from(unit);
        }
        if self.windows().active() == Some(window) && self.windows().focus().is_none() {
            if message.message == 0x105 {
                message.message = 0x104;
            } else if message.message == 0x100 {
                return Ok(User32Outcome32::Return(0));
            }
        }
        let modifiers = u8::from(self.input.queued[0x10] & 0x80 != 0) * 4
            | u8::from(self.input.queued[0x11] & 0x80 != 0) * 8
            | u8::from(self.input.queued[0x12] & 0x80 != 0) * 16;
        let Some(command) = table
            .iter()
            .find(|entry| entry.matches(message.message, message.wparam, modifiers))
            .map(|entry| entry.command)
        else {
            return Ok(User32Outcome32::Return(0));
        };
        if self.pending.len() + 2 > self.continuation_limit {
            return Err(E::Capacity);
        }
        let data = self.state.record(window)?;
        let menu = if data.style & 0x40000000 == 0 && data.menu != 0 {
            let menu = MenuHandle32::try_from(data.menu)?;
            if self.accelerator_menu_path(menu, command)?.is_some() {
                Some(menu)
            } else {
                None
            }
        } else {
            None
        };
        self.start(Continuation32::Accelerator(Dispatch32 {
            window,
            thread: call.thread_id,
            command,
            menu,
            stage: if menu.is_some() && self.windows().capture().is_none() {
                Stage32::Initialize
            } else {
                Stage32::Command
            },
            child: None,
            completed: None,
        }))
    }

    pub(in crate::process) fn pending_accelerator_message(
        &self,
        token: u64,
    ) -> Result<Option<[u32; 4]>, E> {
        let pending = self
            .pending
            .last()
            .filter(|pending| pending.token == token)
            .ok_or(E::InvalidToken)?;
        let Continuation32::Accelerator(dispatch) = pending.continuation else {
            return Ok(None);
        };
        if dispatch.child.is_some() {
            return Err(E::InvalidToken);
        }
        self.accelerator_message(dispatch)
    }

    fn accelerator_message(&self, dispatch: Dispatch32) -> Result<Option<[u32; 4]>, E> {
        let Ok(record) = self.windows().get(dispatch.window) else {
            return Ok(None);
        };
        let menu = dispatch.menu.filter(|&menu| self.menus.contains(menu));
        if let Some(expected) = dispatch.menu
            && (menu != Some(expected)
                || self.state.record(dispatch.window)?.menu != expected.get())
        {
            return Ok(None);
        }
        let event = match dispatch.stage {
            Stage32::Initialize => [
                dispatch.window.get(),
                0x116,
                menu.ok_or(E::InvalidRequest)?.get(),
                0,
            ],
            Stage32::Popup(index) => {
                let Some(path) =
                    self.accelerator_menu_path(menu.ok_or(E::InvalidRequest)?, dispatch.command)?
                else {
                    return Ok(None);
                };
                let Some(popup) = path.get(index).copied().flatten() else {
                    return Ok(None);
                };
                [
                    dispatch.window.get(),
                    0x117,
                    popup.menu.get(),
                    popup.position,
                ]
            }
            Stage32::Command => {
                if !record.description.enabled {
                    return Ok(None);
                }
                if let Some(menu) = menu {
                    let Some(path) = self.accelerator_menu_path(menu, dispatch.command)? else {
                        return Ok(None);
                    };
                    if self.state.record(dispatch.window)?.style & 0x20000000 != 0
                        || self
                            .menus
                            .item(menu, u32::from(dispatch.command), false)?
                            .state
                            & 3
                            != 0
                    {
                        return Ok(None);
                    }
                    let mut parent = menu;
                    for popup in path.into_iter().flatten() {
                        if self.menus.item(parent, popup.position, true)?.state & 3 != 0 {
                            return Ok(None);
                        }
                        parent = popup.menu;
                    }
                }
                [
                    dispatch.window.get(),
                    0x111,
                    u32::from(dispatch.command) | 0x10000,
                    0,
                ]
            }
            Stage32::Finish => return Ok(None),
        };
        Ok(Some(event))
    }

    pub(in crate::process::windows::user32) fn resume_accelerator(
        &mut self,
        token: u64,
    ) -> Result<User32Outcome32, E> {
        let pending = self
            .pending
            .last()
            .filter(|pending| pending.token == token)
            .ok_or(E::InvalidToken)?;
        let Continuation32::Accelerator(mut dispatch) = pending.continuation else {
            return Err(E::InvalidToken);
        };
        let Some(message) = self.accelerator_message(dispatch)? else {
            if matches!(dispatch.stage, Stage32::Popup(_))
                && self.windows().get(dispatch.window).is_ok()
            {
                dispatch.stage = Stage32::Command;
                self.pending.last_mut().unwrap().continuation =
                    Continuation32::Accelerator(dispatch);
                return Ok(User32Outcome32::Wait {
                    continuation: token,
                });
            }
            self.pending.pop();
            return Ok(User32Outcome32::Return(1));
        };
        let outcome = self.send_message(
            &message,
            User32CallContext32 {
                thread_id: dispatch.thread,
                ..self.runtime_input
            },
        )?;
        dispatch.child = Some(match outcome {
            User32Outcome32::Callback(callback) => callback.continuation,
            User32Outcome32::Wait { continuation } => continuation,
            User32Outcome32::Return(_) => return Err(E::InvalidRequest),
        });
        dispatch.completed = None;
        self.pending
            .iter_mut()
            .find(|pending| pending.token == token)
            .ok_or(E::InvalidToken)?
            .continuation = Continuation32::Accelerator(dispatch);
        Ok(outcome)
    }

    pub(in crate::process::windows::user32) fn complete_accelerator_child(
        &mut self,
        child: u64,
        outcome: User32Outcome32,
    ) -> Result<User32Outcome32, E> {
        if !matches!(outcome, User32Outcome32::Return(_)) {
            return Ok(outcome);
        }
        let Some(pending) = self.pending.last_mut() else {
            return Ok(outcome);
        };
        let Continuation32::Accelerator(mut dispatch) = pending.continuation else {
            return Ok(outcome);
        };
        if dispatch.child != Some(child) {
            return Ok(outcome);
        }
        dispatch.stage = match dispatch.stage {
            Stage32::Initialize => Stage32::Popup(0),
            Stage32::Popup(index) => Stage32::Popup(index + 1),
            Stage32::Command => Stage32::Finish,
            Stage32::Finish => return Err(E::InvalidToken),
        };
        dispatch.child = None;
        dispatch.completed = Some(child);
        pending.continuation = Continuation32::Accelerator(dispatch);
        Ok(User32Outcome32::Wait {
            continuation: pending.token,
        })
    }

    pub(in crate::process::windows::user32) fn retry_accelerator_completion(
        &self,
        child: u64,
    ) -> Option<User32Outcome32> {
        let pending = self.pending.last()?;
        let Continuation32::Accelerator(dispatch) = pending.continuation else {
            return None;
        };
        (dispatch.child.is_none() && dispatch.completed == Some(child)).then_some(
            User32Outcome32::Wait {
                continuation: pending.token,
            },
        )
    }

    fn accelerator_menu_path(
        &self,
        menu: MenuHandle32,
        command: u16,
    ) -> Result<Option<[Option<Popup32>; 32]>, E> {
        let mut path = [None; 32];
        let mut visited = [None; 256];
        Ok(self
            .accelerator_find_command(menu, u32::from(command), &mut path, 0, &mut visited)?
            .then_some(path))
    }

    fn accelerator_find_command(
        &self,
        menu: MenuHandle32,
        command: u32,
        path: &mut [Option<Popup32>; 32],
        depth: usize,
        visited: &mut [Option<MenuHandle32>; 256],
    ) -> Result<bool, E> {
        if visited.contains(&Some(menu)) {
            return Ok(false);
        }
        let at = visited
            .iter()
            .position(Option::is_none)
            .ok_or(E::Capacity)?;
        visited[at] = Some(menu);
        for position in 0..self.menus.count(menu)? {
            let item = self.menus.item(menu, position as u32, true)?;
            if item.id == command {
                return Ok(true);
            }
            if let Some(child) = item.submenu.filter(|&child| self.menus.contains(child)) {
                if depth == path.len() {
                    return Err(E::Capacity);
                }
                path[depth] = Some(Popup32 {
                    menu: child,
                    position: position as u32,
                });
                if self.accelerator_find_command(child, command, path, depth + 1, visited)? {
                    return Ok(true);
                }
                path[depth] = None;
            }
        }
        Ok(false)
    }
}
