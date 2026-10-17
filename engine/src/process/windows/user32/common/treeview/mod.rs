pub(in crate::process::windows::user32) mod changes;
mod items;
mod painting;
use super::super::{User32CallContext32, User32Context32, User32Outcome32, memory};
use crate::{
    memory::{AddressSpace, GuestAddress},
    process::{HostError, windows::gdi::GdiRuntime32},
    windows::{
        comctl32::{
            CommonControlState32,
            treeview::{Item32, MAX_TEXT, ROOT, TreeView32},
        },
        gdi::{GdiContext32, Region32},
        user32::{Point32, Rect32, User32Error32 as E},
        window::WindowHandle32,
    },
};
impl User32Context32 {
    fn tree(&self, window: WindowHandle32) -> Result<&TreeView32, E> {
        match self.state.common_state(window)? {
            CommonControlState32::TreeView(tree) => Ok(tree),
            _ => Err(E::Win32(1407)),
        }
    }
    fn tree_mut(&mut self, window: WindowHandle32) -> Result<&mut TreeView32, E> {
        match self.state.common_state_mut(window)? {
            CommonControlState32::TreeView(tree) => Ok(tree),
            _ => Err(E::Win32(1407)),
        }
    }
    fn tree_changed(&mut self, window: WindowHandle32) {
        let extent = self
            .windows()
            .get(window)
            .expect("same tree owner")
            .description
            .extent;
        self.state
            .set_update_region(
                window,
                Region32::rectangle(Rect32 {
                    left: 0,
                    top: 0,
                    right: extent.width as i32,
                    bottom: extent.height as i32,
                }),
                true,
            )
            .expect("tree paint preflighted");
    }
    pub(super) fn treeview_control_message(
        &mut self,
        args: &[u32],
        wide: bool,
        ram: &mut AddressSpace,
        mut gdi: Option<&mut GdiRuntime32>,
        call: User32CallContext32,
    ) -> Result<User32Outcome32, HostError> {
        let window = self
            .owned(args[0], call.thread_id)
            .map_err(HostError::User32)?;
        let value = match args[1] {
            0x81 => {
                self.tree_mut(window).map_err(HostError::User32)?.unicode = wide;
                1
            }
            1 => 0,
            2 => {
                return self
                    .begin_tree_change(window, changes::Change32::Delete(ROOT), 0, false, ram, call)
                    .map_err(HostError::User32);
            }
            0xf | 0x318 => {
                return self.paint_tree(
                    window,
                    args[2],
                    args[1] == 0x318,
                    ram,
                    gdi.ok_or(HostError::InvalidRequest)?,
                    call.thread_id,
                );
            }
            0x14 => {
                if args[2] == 0 {
                    0
                } else {
                    self.draw_tree_background(
                        window,
                        args[2],
                        ram,
                        gdi.ok_or(HostError::InvalidRequest)?,
                    )?;
                    1
                }
            }
            0x87 => 0x81,
            7 | 8 | 5 => {
                self.state.check_paint_change().map_err(HostError::User32)?;
                self.tree_changed(window);
                0
            }
            0x30 => {
                if args[2] != 0 {
                    gdi.as_ref()
                        .ok_or(HostError::InvalidRequest)?
                        .font_metrics(args[2])
                        .map_err(HostError::Gdi)?;
                }
                if args[3] != 0 {
                    self.state.check_paint_change().map_err(HostError::User32)?;
                }
                self.tree_mut(window).map_err(HostError::User32)?.font = args[2];
                if args[3] != 0 {
                    self.tree_changed(window);
                }
                0
            }
            0x31 => self.tree(window).map_err(HostError::User32)?.font,
            0x1100 | 0x1132 => {
                return self
                    .insert_tree_item(window, args[3], args[1] == 0x1132, ram, call)
                    .map(User32Outcome32::Return)
                    .map_err(HostError::User32);
            }
            0x1101 => {
                return self
                    .begin_tree_change(
                        window,
                        changes::Change32::Delete(if args[3] == 0 { ROOT } else { args[3] }),
                        0,
                        false,
                        ram,
                        call,
                    )
                    .map_err(HostError::User32);
            }
            0x1102 => {
                let item = args[3];
                let Some(node) = self.tree(window).map_err(HostError::User32)?.item(item) else {
                    return Ok(User32Outcome32::Return(0));
                };
                let expanded = match args[2] {
                    1 => false,
                    2 => true,
                    3 => node.state & 0x20 == 0,
                    _ => return Err(HostError::User32(E::Win32(50))),
                };
                return self
                    .begin_tree_change(
                        window,
                        changes::Change32::Expand(item, expanded),
                        0,
                        false,
                        ram,
                        call,
                    )
                    .map_err(HostError::User32);
            }
            0x1104 | 0x1110 | 0x1111 => {
                return self
                    .tree_geometry_message(
                        window,
                        args,
                        ram,
                        gdi.as_deref_mut().ok_or(HostError::InvalidRequest)?,
                        call.thread_id,
                    )
                    .map(User32Outcome32::Return);
            }
            0x1105 => self.tree(window).map_err(HostError::User32)?.items.len() as u32,
            0x1106 => self.tree(window).map_err(HostError::User32)?.indent as u32,
            0x1107 => {
                self.state.check_paint_change().map_err(HostError::User32)?;
                self.tree_mut(window).map_err(HostError::User32)?.indent =
                    (args[2] as i32).clamp(1, 32767);
                self.tree_changed(window);
                0
            }
            0x1108 => 0,
            0x1109 => {
                if args[3] != 0 {
                    return Err(HostError::User32(E::Win32(50)));
                }
                0
            }
            0x110a => self
                .tree_next_item(window, args[2], args[3])
                .map_err(HostError::User32)?,
            0x110b => match args[2] {
                9 => {
                    return self
                        .begin_tree_change(
                            window,
                            changes::Change32::Select(args[3]),
                            0,
                            false,
                            ram,
                            call,
                        )
                        .map_err(HostError::User32);
                }
                8 => {
                    if args[3] != 0
                        && self
                            .tree(window)
                            .map_err(HostError::User32)?
                            .item(args[3])
                            .is_none()
                    {
                        0
                    } else {
                        self.state.check_paint_change().map_err(HostError::User32)?;
                        self.tree_mut(window)
                            .map_err(HostError::User32)?
                            .drop_target = args[3];
                        self.tree_changed(window);
                        1
                    }
                }
                5 => {
                    if !self
                        .tree(window)
                        .map_err(HostError::User32)?
                        .visible(args[3])
                    {
                        0
                    } else {
                        self.state.check_paint_change().map_err(HostError::User32)?;
                        self.tree_mut(window).map_err(HostError::User32)?.first = args[3];
                        self.tree_changed(window);
                        1
                    }
                }
                _ => return Err(HostError::User32(E::Win32(50))),
            },
            0x110c | 0x113e => {
                return self
                    .get_tree_item(window, args[3], args[1] == 0x113e, ram, call)
                    .map(User32Outcome32::Return)
                    .map_err(HostError::User32);
            }
            0x110d | 0x113f => {
                return self
                    .set_tree_item(window, args[3], args[1] == 0x113f, ram, call)
                    .map(User32Outcome32::Return)
                    .map_err(HostError::User32);
            }
            0x110e | 0x1141 | 0x1112 | 0x1113 | 0x1115 | 0x1118 => {
                return Err(HostError::User32(E::Win32(50)));
            }
            0x1114 => {
                return self
                    .begin_tree_change(
                        window,
                        changes::Change32::Ensure(args[3]),
                        0,
                        false,
                        ram,
                        call,
                    )
                    .map_err(HostError::User32);
            }
            0x111b => {
                let old = self.tree(window).map_err(HostError::User32)?.item_height;
                self.state.check_paint_change().map_err(HostError::User32)?;
                self.tree_mut(window)
                    .map_err(HostError::User32)?
                    .item_height = if args[2] as i32 == -1 {
                    0
                } else {
                    (args[2] as i32).clamp(1, 32767)
                };
                self.tree_changed(window);
                old as u32
            }
            0x111c => self
                .tree_row_height(window, gdi.as_deref().ok_or(HostError::InvalidRequest)?)
                .map_err(HostError::Gdi)? as u32,
            0x111d | 0x111e => {
                self.state.check_paint_change().map_err(HostError::User32)?;
                let tree = self.tree_mut(window).map_err(HostError::User32)?;
                let slot = if args[1] == 0x111d {
                    &mut tree.background
                } else {
                    &mut tree.text_color
                };
                let old = *slot;
                *slot = args[3];
                self.tree_changed(window);
                old
            }
            0x111f => self.tree(window).map_err(HostError::User32)?.background,
            0x1120 => self.tree(window).map_err(HostError::User32)?.text_color,
            0x1127 => {
                let tree = self.tree(window).map_err(HostError::User32)?;
                tree.item(args[2])
                    .map_or(0, |item| tree.state(item) & args[3])
            }
            0x2005 => {
                let old = self.tree(window).map_err(HostError::User32)?.unicode;
                self.tree_mut(window).map_err(HostError::User32)?.unicode = args[2] != 0;
                u32::from(old)
            }
            0x2006 => u32::from(self.tree(window).map_err(HostError::User32)?.unicode),
            0x100 | 0x201 | 0x203 | 0x115 | 0x20a => {
                return self.tree_input(
                    window,
                    args,
                    ram,
                    gdi.ok_or(HostError::InvalidRequest)?,
                    call,
                );
            }
            _ => {
                return self
                    .def_window_proc(args, ram, call)
                    .map_err(HostError::User32);
            }
        };
        Ok(User32Outcome32::Return(value))
    }
    fn tree_next_item(&self, window: WindowHandle32, kind: u32, id: u32) -> Result<u32, E> {
        let tree = self.tree(window)?;
        let node = tree.item(id);
        Ok(match kind {
            0 => tree.first_child(0),
            3 => node.map_or(0, |item| item.parent),
            1 | 2 => {
                if let Some(item) = node {
                    let index = tree.items.iter().position(|item| item.id == id).unwrap();
                    if kind == 1 {
                        tree.items[index + 1..]
                            .iter()
                            .find(|other| other.parent == item.parent)
                            .map_or(0, |item| item.id)
                    } else {
                        tree.items[..index]
                            .iter()
                            .rev()
                            .find(|other| other.parent == item.parent)
                            .map_or(0, |item| item.id)
                    }
                } else {
                    0
                }
            }
            4 => tree.first_child(id),
            5 => {
                let ids = tree.visible_ids()?;
                if ids.contains(&tree.first) {
                    tree.first
                } else {
                    ids.first().copied().unwrap_or(0)
                }
            }
            6 | 7 | 10 => {
                let ids = tree.visible_ids()?;
                if kind == 10 {
                    ids.last().copied().unwrap_or(0)
                } else {
                    ids.iter()
                        .position(|&entry| entry == id)
                        .and_then(|index| {
                            if kind == 6 {
                                ids.get(index + 1)
                            } else {
                                index.checked_sub(1).and_then(|index| ids.get(index))
                            }
                        })
                        .copied()
                        .unwrap_or(0)
                }
            }
            8 => tree.drop_target,
            9 => tree.selected,
            _ => 0,
        })
    }
}
