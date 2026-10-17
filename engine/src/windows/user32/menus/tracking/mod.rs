use super::{MenuHandle32, MenuStore32};
use crate::windows::user32::User32Error32 as E;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MenuLevel32 {
    pub menu: MenuHandle32,
    pub selected: Option<usize>,
}

#[derive(Clone, Copy)]
pub(crate) struct MenuTracking32 {
    levels: [Option<MenuLevel32>; 32],
    depth: usize,
    ended: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuInput32 {
    Next(bool),
    Home,
    End,
    Open,
    Forward,
    Previous,
    Back,
    Cancel,
    Mnemonic(u16),
    Point {
        depth: usize,
        position: Option<usize>,
        choose: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuAction32 {
    None,
    Select {
        depth: usize,
        position: Option<usize>,
    },
    Open {
        depth: usize,
        menu: MenuHandle32,
        parent_position: usize,
    },
    Close {
        depth: usize,
    },
    Command {
        menu: MenuHandle32,
        position: usize,
        id: u32,
    },
    Cancel,
    Unmatched(u16),
}

impl MenuTracking32 {
    pub(crate) fn new(menus: &MenuStore32, root: MenuHandle32) -> Result<Self, E> {
        menus.count(root)?;
        let mut levels = [None; 32];
        levels[0] = Some(MenuLevel32 {
            menu: root,
            selected: None,
        });
        Ok(Self {
            levels,
            depth: 1,
            ended: false,
        })
    }

    pub(crate) fn depth(&self) -> usize {
        self.depth
    }

    pub(crate) fn level(&self, depth: usize) -> Option<MenuLevel32> {
        self.levels
            .get(depth)
            .copied()
            .flatten()
            .filter(|_| depth < self.depth)
    }

    pub(crate) fn input(
        &mut self,
        menus: &MenuStore32,
        input: MenuInput32,
    ) -> Result<MenuAction32, E> {
        let mut next = *self;
        let action = next.input_step(menus, input)?;
        *self = next;
        Ok(action)
    }

    fn input_step(&mut self, menus: &MenuStore32, input: MenuInput32) -> Result<MenuAction32, E> {
        if self.ended {
            return Err(E::InvalidRequest);
        }
        let depth = self.depth - 1;
        let level = self.levels[depth].ok_or(E::InvalidRequest)?;
        let count = menus.count(level.menu)?;
        match input {
            MenuInput32::Cancel => {
                self.ended = true;
                Ok(MenuAction32::Cancel)
            }
            MenuInput32::Back => {
                if depth == 0 {
                    self.ended = true;
                    Ok(MenuAction32::Cancel)
                } else {
                    self.truncate(depth);
                    Ok(MenuAction32::Close { depth })
                }
            }
            MenuInput32::Next(_) | MenuInput32::Home | MenuInput32::End => {
                let backwards =
                    matches!(input, MenuInput32::End) || matches!(input, MenuInput32::Next(true));
                let initial = match input {
                    MenuInput32::Home => 0,
                    MenuInput32::End => count.saturating_sub(1),
                    _ => level.selected.map_or(
                        if backwards {
                            count.saturating_sub(1)
                        } else {
                            0
                        },
                        |at| {
                            if backwards {
                                if at == 0 {
                                    count.saturating_sub(1)
                                } else {
                                    at - 1
                                }
                            } else {
                                (at + 1) % count.max(1)
                            }
                        },
                    ),
                };
                for step in 0..count {
                    let at = if backwards {
                        (initial + count - step) % count
                    } else {
                        (initial + step) % count
                    };
                    if menus.item(level.menu, at as u32, true)?.kind & 0x800 == 0 {
                        return Ok(self.select(depth, Some(at)));
                    }
                }
                Ok(MenuAction32::None)
            }
            MenuInput32::Open => self.activate(menus, depth),
            MenuInput32::Forward => {
                if level.selected.is_some_and(|at| {
                    menus
                        .item(level.menu, at as u32, true)
                        .is_ok_and(|item| item.submenu.is_some())
                }) {
                    self.activate(menus, depth)
                } else {
                    self.column(menus, depth, false)
                }
            }
            MenuInput32::Previous => {
                if depth == 0 {
                    self.column(menus, depth, true)
                } else {
                    self.truncate(depth);
                    Ok(MenuAction32::Close { depth })
                }
            }
            MenuInput32::Point {
                depth,
                position,
                choose,
            } => {
                let level = self.level(depth).ok_or(E::InvalidRequest)?;
                if let Some(at) = position {
                    if menus.item(level.menu, at as u32, true)?.kind & 0x800 != 0 {
                        return Ok(MenuAction32::None);
                    }
                }
                let action = self.select(depth, position);
                if choose {
                    self.activate(menus, depth)
                } else {
                    Ok(action)
                }
            }
            MenuInput32::Mnemonic(unit) => {
                let wanted = crate::windows::text::ordinal_key(&[unit]);
                let mut first = None;
                let mut after = None;
                let mut matches = 0;
                for at in 0..count {
                    let item = menus.item(level.menu, at as u32, true)?;
                    if item.kind & 0x800 != 0 || item.state & 3 != 0 {
                        continue;
                    }
                    if mnemonic(&item.text)
                        .is_some_and(|key| crate::windows::text::ordinal_key(&[key]) == wanted)
                    {
                        first.get_or_insert(at);
                        if level.selected.is_some_and(|selected| at > selected) {
                            after.get_or_insert(at);
                        }
                        matches += 1;
                    }
                }
                let Some(at) = after.or(first) else {
                    return Ok(MenuAction32::Unmatched(unit));
                };
                let action = self.select(depth, Some(at));
                if matches == 1 {
                    self.activate(menus, depth)
                } else {
                    Ok(action)
                }
            }
        }
    }

    fn select(&mut self, depth: usize, position: Option<usize>) -> MenuAction32 {
        if self.levels[depth].unwrap().selected == position {
            return MenuAction32::None;
        }
        self.truncate(depth + 1);
        self.levels[depth].as_mut().unwrap().selected = position;
        MenuAction32::Select { depth, position }
    }

    fn column(
        &mut self,
        menus: &MenuStore32,
        depth: usize,
        backwards: bool,
    ) -> Result<MenuAction32, E> {
        let level = self.level(depth).ok_or(E::InvalidRequest)?;
        let count = menus.count(level.menu)?;
        let selected = level.selected.unwrap_or(0).min(count.saturating_sub(1));
        let mut start = 0;
        let mut previous = None;
        for at in 1..=selected {
            if menus.item(level.menu, at as u32, true)?.kind & 0x60 != 0 {
                previous = Some(start);
                start = at;
            }
        }
        let target = if backwards {
            previous
        } else {
            let mut next = None;
            for at in selected + 1..count {
                if menus.item(level.menu, at as u32, true)?.kind & 0x60 != 0 {
                    next = Some(at);
                    break;
                }
            }
            next
        };
        let Some(target) = target else {
            return Ok(MenuAction32::None);
        };
        for at in target..count {
            let item = menus.item(level.menu, at as u32, true)?;
            if at != target && item.kind & 0x60 != 0 {
                break;
            }
            if item.kind & 0x800 == 0 {
                return Ok(self.select(depth, Some(at)));
            }
        }
        Ok(MenuAction32::None)
    }

    fn truncate(&mut self, depth: usize) {
        self.levels[depth..].fill(None);
        self.depth = depth;
    }

    fn activate(&mut self, menus: &MenuStore32, depth: usize) -> Result<MenuAction32, E> {
        let level = self.level(depth).ok_or(E::InvalidRequest)?;
        let Some(at) = level.selected else {
            return Ok(MenuAction32::None);
        };
        let item = menus.item(level.menu, at as u32, true)?;
        if item.state & 3 != 0 || item.kind & 0x800 != 0 {
            return Ok(MenuAction32::None);
        }
        if let Some(menu) = item.submenu {
            menus.count(menu)?;
            let next = depth + 1;
            if next == self.levels.len() {
                return Err(E::Capacity);
            }
            self.truncate(next);
            self.levels[next] = Some(MenuLevel32 {
                menu,
                selected: None,
            });
            self.depth = next + 1;
            Ok(MenuAction32::Open {
                depth: next,
                menu,
                parent_position: at,
            })
        } else {
            self.ended = true;
            Ok(MenuAction32::Command {
                menu: level.menu,
                position: at,
                id: item.id,
            })
        }
    }
}

fn mnemonic(text: &[u16]) -> Option<u16> {
    let mut at = 0;
    while at < text.len() {
        if matches!(text[at], 8 | 9) {
            return None;
        }
        if text[at] == u16::from(b'&') {
            at += 1;
            let &next = text.get(at)?;
            if next != u16::from(b'&') {
                return Some(next);
            }
        }
        at += 1;
    }
    None
}

#[cfg(test)]
mod tests;
