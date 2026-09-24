use std::collections::BTreeMap;

use super::{
    DispatchError, GuestMemory, MemoryError, Register32, callbacks, gdi, thread,
    user_atoms::{self, UserAtoms},
};

pub(super) const DESKTOP: u32 = 1;

#[derive(Clone, Copy)]
pub(super) struct DestroyPending {
    handle: u32,
    procedure: u32,
    nc_destroy: bool,
}

#[derive(Clone, Copy)]
pub(super) enum Call {
    Desktop,
    Find,
    IsWindow,
    Active,
    DlgItem,
    Top,
    Window,
}

impl Call {
    pub(super) fn at(offset: u32) -> Option<Self> {
        match offset {
            0x10 => Some(Self::Desktop),
            0x26c => Some(Self::Find),
            0x270 => Some(Self::IsWindow),
            0x430 => Some(Self::Active),
            0x438 => Some(Self::DlgItem),
            0x454 => Some(Self::Top),
            0x458 => Some(Self::Window),
            _ => None,
        }
    }
    pub(super) fn arguments(self) -> usize {
        match self {
            Self::Desktop | Self::Active => 0,
            Self::IsWindow | Self::Top => 1,
            Self::Find | Self::DlgItem | Self::Window => 2,
        }
    }
}

#[derive(Default)]
pub(super) struct Window {
    pub(super) class: u32,
    pub(super) instance: u32,
    pub(super) procedure: u32,
    pub(super) style: u32,
    pub(super) exstyle: u32,
    pub(super) title: String,
    pub(super) rectangle: [i32; 4],
    pub(super) client: [i32; 4],
    pub(super) icons: [u32; 2],
    pub(super) parent: u32,
    pub(super) id: u32,
    pub(super) dialog_units: Option<[i16; 4]>,
    pub(super) dialog_result: Option<u32>,
    pub(super) needs_paint: bool,
    pub(super) topmost: bool,
    pub(super) combo: ComboBox,
}

#[derive(Default)]
pub(super) struct ComboBox {
    items: Vec<ComboItem>,
    bytes: usize,
    selected: Option<usize>,
}

struct ComboItem {
    title: String,
    data: u32,
}

impl ComboBox {
    fn add(&mut self, style: u32, title: String) -> u32 {
        let Some(bytes) = self.bytes.checked_add(title.len() + 1) else {
            return u32::MAX - 1;
        };
        if self.items.len() == 1024 || bytes > 64 * 1024 {
            return u32::MAX - 1;
        }
        let index = if style & 0x100 == 0 {
            self.items.len()
        } else {
            self.items.partition_point(|item| {
                item.title
                    .bytes()
                    .map(|byte| byte.to_ascii_lowercase())
                    .cmp(title.bytes().map(|byte| byte.to_ascii_lowercase()))
                    != std::cmp::Ordering::Greater
            })
        };
        if let Some(selected) = &mut self.selected
            && index <= *selected
        {
            *selected += 1;
        }
        self.items.insert(index, ComboItem { title, data: 0 });
        self.bytes = bytes;
        u32::try_from(index).expect("bounded combo count")
    }

    fn count(&self) -> u32 {
        u32::try_from(self.items.len()).expect("bounded combo count")
    }

    fn item_data(&self, index: u32) -> u32 {
        self.items
            .get(index as usize)
            .map_or(u32::MAX, |item| item.data)
    }

    fn set_item_data(&mut self, index: u32, data: u32) -> u32 {
        let Some(item) = self.items.get_mut(index as usize) else {
            return u32::MAX;
        };
        item.data = data;
        0
    }

    fn select(&mut self, index: u32) -> u32 {
        self.selected = ((index as usize) < self.items.len()).then_some(index as usize);
        self.selection()
    }

    fn selection(&self) -> u32 {
        self.selected.map_or(u32::MAX, |index| {
            u32::try_from(index).expect("bounded combo count")
        })
    }

    fn selected_title(&self) -> Option<&str> {
        self.selected.map(|index| self.items[index].title.as_str())
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowSnapshot {
    pub hwnd: u32,
    pub parent: u32,
    pub id: u32,
    pub class: u32,
    pub style: u32,
    pub exstyle: u32,
    pub title: String,
    pub rectangle: [i32; 4],
    pub client: [i32; 4],
    pub dialog_units: Option<[i16; 4]>,
    pub active: bool,
}

pub(super) struct Desktop {
    top_levels: BTreeMap<u32, Window>,
    children: BTreeMap<u32, Window>,
    active: u32,
    activation_epoch: Option<u64>,
    next: u32,
}

impl Default for Desktop {
    fn default() -> Self {
        Self {
            top_levels: BTreeMap::new(),
            children: BTreeMap::new(),
            active: 0,
            activation_epoch: Some(0),
            next: 0x7500_0004,
        }
    }
}

impl Desktop {
    pub(super) fn activation(&self) -> (u32, Option<u64>) {
        (self.active, self.activation_epoch)
    }

    fn set_active(&mut self, handle: u32) {
        if self.active != handle {
            self.activation_epoch = self.activation_epoch.and_then(|epoch| epoch.checked_add(1));
            self.active = handle;
        }
    }

    pub(super) fn top_window(&self, parent: u32) -> u32 {
        if matches!(parent, 0 | DESKTOP) {
            return self.top_levels.keys().next_back().copied().unwrap_or(0);
        }
        self.children
            .iter()
            .rev()
            .find_map(|(&handle, window)| (window.parent == parent).then_some(handle))
            .unwrap_or(0)
    }

    pub(super) fn relative_window(&self, handle: u32, relation: u32) -> Option<u32> {
        let parent = self.window(handle)?.parent;
        if relation == 5 {
            return Some(self.top_window(handle));
        }
        if relation == 4 {
            return Some(0);
        }
        let siblings: Vec<u32> = if parent == 0 {
            self.top_levels.keys().copied().collect()
        } else {
            self.children
                .iter()
                .filter_map(|(&handle, window)| (window.parent == parent).then_some(handle))
                .collect()
        };
        let index = siblings.binary_search(&handle).ok()?;
        Some(match relation {
            0 => *siblings.last().expect("selected window exists"),
            1 => siblings[0],
            2 => index
                .checked_sub(1)
                .and_then(|index| siblings.get(index))
                .copied()
                .unwrap_or(0),
            3 => siblings.get(index + 1).copied().unwrap_or(0),
            _ => return None,
        })
    }

    pub(super) fn snapshots(&self) -> Vec<WindowSnapshot> {
        let mut snapshots: Vec<_> = self
            .top_levels
            .iter()
            .chain(&self.children)
            .map(|(&hwnd, window)| WindowSnapshot {
                hwnd,
                parent: window.parent,
                id: window.id,
                class: window.class,
                style: window.style,
                exstyle: window.exstyle,
                title: window.title.clone(),
                rectangle: window.rectangle,
                client: window.client,
                dialog_units: window.dialog_units,
                active: hwnd == self.active,
            })
            .collect();
        snapshots.sort_unstable_by_key(|window| window.hwnd);
        snapshots
    }

    pub(super) fn available(&self) -> Option<u32> {
        self.available_many(1)
    }
    pub(super) fn available_many(&self, count: usize) -> Option<u32> {
        let remaining = 4096_usize.checked_sub(self.top_levels.len() + self.children.len())?;
        let last = self
            .next
            .checked_add(u32::try_from(count.checked_sub(1)?).ok()?.checked_mul(4)?)?;
        (count <= remaining && last <= 0x75ff_fffc).then_some(self.next)
    }
    pub(super) fn insert(&mut self, handle: u32, window: Window) {
        self.top_levels.insert(handle, window);
        self.next += 4;
    }
    pub(super) fn insert_child(&mut self, handle: u32, window: Window) {
        self.children.insert(handle, window);
        self.next += 4;
    }
    pub(super) fn window(&self, handle: u32) -> Option<&Window> {
        self.top_levels
            .get(&handle)
            .or_else(|| self.children.get(&handle))
    }
    pub(super) fn window_mut(&mut self, handle: u32) -> Option<&mut Window> {
        if let Some(window) = self.top_levels.get_mut(&handle) {
            Some(window)
        } else {
            self.children.get_mut(&handle)
        }
    }
    pub(super) fn child(&self, parent: u32, id: u32) -> Option<u32> {
        self.children.iter().find_map(|(&handle, window)| {
            (window.parent == parent && window.id == id).then_some(handle)
        })
    }
    pub(super) fn remove(&mut self, handle: u32) {
        self.top_levels.remove(&handle);
        self.remove_children(handle);
        if self.active == handle {
            self.set_active(0);
        }
    }
    pub(super) fn remove_children(&mut self, handle: u32) {
        self.children.retain(|_, window| window.parent != handle);
    }
    pub(super) fn activate_created(&mut self, handle: u32) {
        if self
            .top_levels
            .get(&handle)
            .is_some_and(|window| window.style & 0x1000_0000 != 0)
        {
            self.set_active(handle);
        }
    }
    pub(super) fn activate_foreground(&mut self, handle: u32) -> bool {
        if self
            .top_levels
            .get(&handle)
            .is_none_or(|window| window.style & 0x1000_0000 == 0)
        {
            return false;
        }
        self.set_active(handle);
        true
    }
    pub(super) fn show_activated(&mut self, handle: u32) -> Option<u32> {
        let window = self.top_levels.get_mut(&handle)?;
        let was_visible = u32::from(window.style & 0x1000_0000 != 0);
        if was_visible == 0 && window.class == 0x8002 && window.dialog_units.is_some() {
            window.needs_paint = true;
        }
        window.style |= 0x1000_0000;
        self.set_active(handle);
        Some(was_visible)
    }
    pub(super) fn hide_window(&mut self, handle: u32) -> Option<u32> {
        let window = self.top_levels.get_mut(&handle)?;
        let was_visible = u32::from(window.style & 0x1000_0000 != 0);
        window.style &= !0x1000_0000;
        window.needs_paint = false;
        if self.active == handle {
            self.set_active(0);
        }
        Some(was_visible)
    }
    pub(super) fn invalidate_full(&mut self, handle: u32) -> bool {
        let Some(window) = self.top_levels.get_mut(&handle) else {
            return false;
        };
        window.needs_paint = true;
        true
    }
    pub(super) fn position_topmost_no_redraw(&mut self, args: &[u32]) -> bool {
        let [handle, insert_after, x, y, width, height, flags] =
            <[u32; 7]>::try_from(args).expect("SetWindowPos arity");
        if insert_after != u32::MAX
            || flags != 8
            || self.active != handle
            || self.top_levels.len() != 1
            || width == 0
            || height == 0
            || x.checked_add(width)
                .is_none_or(|right| right > gdi::SCREEN_WIDTH)
            || y.checked_add(height)
                .is_none_or(|bottom| bottom > gdi::SCREEN_HEIGHT)
        {
            return false;
        }
        let Some(window) = self.top_levels.get_mut(&handle) else {
            return false;
        };
        if window.dialog_units.is_some() {
            return false;
        }
        let old = window.rectangle;
        let client = window.client;
        let insets = [
            client[0] - old[0],
            client[1] - old[1],
            old[2] - client[2],
            old[3] - client[3],
        ];
        let (x, y, right, bottom) = (
            x.cast_signed(),
            y.cast_signed(),
            (x + width).cast_signed(),
            (y + height).cast_signed(),
        );
        let new_client = [
            x + insets[0],
            y + insets[1],
            right - insets[2],
            bottom - insets[3],
        ];
        if insets.iter().any(|inset| *inset < 0)
            || new_client[0] >= new_client[2]
            || new_client[1] >= new_client[3]
        {
            return false;
        }
        window.rectangle = [x, y, right, bottom];
        window.client = new_client;
        window.topmost = true;
        true
    }
    pub(super) fn end_dialog(&mut self, handle: u32, result: u32) {
        let window = self.top_levels.get_mut(&handle).expect("validated dialog");
        window.dialog_result = Some(result);
        self.hide_dialog(handle);
    }
    pub(super) fn hide_dialog(&mut self, handle: u32) {
        self.hide_window(handle).expect("validated dialog");
    }
    pub(super) fn has_class(&self, instance: u32, atom: u32) -> bool {
        self.top_levels
            .values()
            .any(|window| window.instance == instance && window.class == atom)
    }
    pub(super) fn dispatch(
        &self,
        call: Call,
        args: &[u32],
        atoms: &UserAtoms,
        memory: &GuestMemory,
    ) -> Result<u32, DispatchError> {
        match call {
            Call::Desktop => Ok(DESKTOP),
            Call::Active => Ok(self.active),
            Call::IsWindow => Ok(u32::from(
                args[0] == DESKTOP
                    || self.top_levels.contains_key(&args[0])
                    || self.children.contains_key(&args[0]),
            )),
            Call::Top => Ok(self.top_window(args[0])),
            Call::Window => Ok(self.relative_window(args[0], args[1]).unwrap_or(0)),
            Call::DlgItem => Ok(self.child(args[0], args[1]).unwrap_or(0)),
            Call::Find => {
                let class = match args[0] {
                    0 => None,
                    1..=0xbfff => return Err(DispatchError::Unsupported),
                    0xc000..=0xffff => Some(args[0]),
                    pointer => Some(
                        atoms
                            .lookup(&user_atoms::read_name(memory, pointer)?)
                            .unwrap_or(0),
                    ),
                };
                let title = if args[1] == 0 {
                    None
                } else {
                    Some(read_title(memory, args[1])?)
                };
                Ok(self
                    .top_levels
                    .iter()
                    .rev()
                    .find_map(|(&handle, window)| {
                        (class.is_none_or(|atom| atom == window.class)
                            && title
                                .as_ref()
                                .is_none_or(|title| title.eq_ignore_ascii_case(&window.title)))
                        .then_some(handle)
                    })
                    .unwrap_or(0))
            }
        }
    }
}

pub(super) fn read_title(memory: &GuestMemory, pointer: u32) -> Result<String, DispatchError> {
    if pointer == 0 {
        return Ok(String::new());
    }
    let mut title = String::new();
    for offset in 0..4096 {
        let address = pointer
            .checked_add(offset)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut byte = [0];
        memory.read(u64::from(address), &mut byte)?;
        if byte[0] == 0 {
            return Ok(title);
        }
        if offset == 4095 || !byte[0].is_ascii() {
            return Err(DispatchError::Unsupported);
        }
        title.push(char::from(byte[0]));
    }
    unreachable!()
}

impl super::Process32 {
    pub(super) fn destroy_dialog(&mut self, handle: u32) -> Result<bool, DispatchError> {
        let Some(window) = self.desktop.window(handle) else {
            thread::Teb(self.cpu.fs_base()).set_last_error(&mut self.memory, 1400)?;
            self.cpu.set_register(Register32::Eax, 0);
            return Ok(false);
        };
        if window.parent != 0
            || window.class != 0x8002
            || window.dialog_units.is_none()
            || window.procedure == 0
        {
            return Err(DispatchError::Unsupported);
        }
        let pending = DestroyPending {
            handle,
            procedure: window.procedure,
            nc_destroy: false,
        };
        let stack = self.cpu.register(Register32::Esp);
        self.threads
            .state_mut(self.cpu.fs_base())?
            .callbacks
            .enter(
                &mut self.cpu,
                &mut self.memory,
                callbacks::Frame {
                    stack,
                    caller: stack,
                    cleanup: 8,
                    creation: None,
                    cbt_hook: None,
                    module: None,
                    dialog: None,
                    destroy: Some(pending),
                    paint: false,
                    sound_enumeration: false,
                },
                pending.procedure,
                &[handle, 2, 0, 0],
            )?;
        self.desktop.hide_dialog(handle);
        Ok(true)
    }

    pub(super) fn finish_destroy_callback(
        &mut self,
        frame: callbacks::Frame,
        pending: DestroyPending,
    ) -> Result<(), DispatchError> {
        if !pending.nc_destroy {
            self.desktop.remove_children(pending.handle);
            let next = DestroyPending {
                nc_destroy: true,
                ..pending
            };
            return self
                .threads
                .state_mut(self.cpu.fs_base())?
                .callbacks
                .replace(
                    &mut self.cpu,
                    &mut self.memory,
                    callbacks::Frame {
                        destroy: Some(next),
                        ..frame
                    },
                    pending.procedure,
                    &[pending.handle, 0x82, 0, 0],
                );
        }
        self.threads
            .state_mut(self.cpu.fs_base())?
            .callbacks
            .finish(&mut self.cpu, &self.memory)?;
        self.desktop.remove(pending.handle);
        self.cpu.set_register(Register32::Eax, 1);
        Ok(())
    }

    pub(super) fn set_dialog_window_pos(&mut self, args: &[u32]) -> Result<(), DispatchError> {
        let Some(window) = self.desktop.window(args[0]) else {
            thread::Teb(self.cpu.fs_base()).set_last_error(&mut self.memory, 1400)?;
            self.cpu.set_register(Register32::Eax, 0);
            return Ok(());
        };
        let (parent, class, is_dialog) =
            (window.parent, window.class, window.dialog_units.is_some());
        if !is_dialog && self.desktop.position_topmost_no_redraw(args) {
            self.cpu.set_register(Register32::Eax, 1);
            return Ok(());
        }
        if parent != 0 || class != 0x8002 || !is_dialog || args[1..] != [0, 0, 0, 0, 0, 0x97] {
            return Err(DispatchError::Unsupported);
        }
        self.desktop.hide_dialog(args[0]);
        self.cpu.set_register(Register32::Eax, 1);
        Ok(())
    }

    pub(super) fn end_dialog(&mut self, args: &[u32]) -> Result<(), DispatchError> {
        let Some(window) = self.desktop.window(args[0]) else {
            thread::Teb(self.cpu.fs_base()).set_last_error(&mut self.memory, 1400)?;
            self.cpu.set_register(Register32::Eax, 0);
            return Ok(());
        };
        if window.parent != 0 || window.class != 0x8002 || window.dialog_units.is_none() {
            return Err(DispatchError::Unsupported);
        }
        self.desktop.end_dialog(args[0], args[1]);
        self.cpu.set_register(Register32::Eax, 1);
        Ok(())
    }

    pub(super) fn enable_dialog_control(&mut self, args: &[u32]) -> Result<(), DispatchError> {
        let Some(window) = self.desktop.window(args[0]) else {
            thread::Teb(self.cpu.fs_base()).set_last_error(&mut self.memory, 1400)?;
            self.cpu.set_register(Register32::Eax, 0);
            return Ok(());
        };
        if window.parent == 0
            || window.dialog_units.is_none()
            || !(0x80..=0x85).contains(&window.class)
            || window.procedure != 0
        {
            return Err(DispatchError::Unsupported);
        }
        let was_disabled = u32::from(window.style & 0x0800_0000 != 0);
        let window = self.desktop.window_mut(args[0]).expect("validated window");
        if args[1] == 0 {
            window.style |= 0x0800_0000;
        } else {
            window.style &= !0x0800_0000;
        }
        self.cpu.set_register(Register32::Eax, was_disabled);
        Ok(())
    }

    pub(super) fn combo_message(&mut self, args: &[u32]) -> Result<Option<u32>, DispatchError> {
        let window = self.desktop.window(args[0]).expect("validated window");
        if window.parent == 0
            || window.dialog_units.is_none()
            || window.class != 0x85
            || (window.style & 0x30 != 0 && window.style & 0x200 == 0)
        {
            return Ok(None);
        }
        match args[1] {
            0x143 if args[3] != 0 => {
                let title = read_title(&self.memory, args[3])?;
                let window = self.desktop.window_mut(args[0]).expect("validated window");
                Ok(Some(window.combo.add(window.style, title)))
            }
            0x14b if args[2..] == [0, 0] => {
                let window = self.desktop.window_mut(args[0]).expect("validated window");
                window.combo.reset();
                window.title.clear();
                Ok(Some(0))
            }
            0x146 if args[2..] == [0, 0] => Ok(Some(window.combo.count())),
            0x147 => Ok(Some(window.combo.selection())),
            0x150 => Ok(Some(window.combo.item_data(args[2]))),
            0x151 => {
                let window = self.desktop.window_mut(args[0]).expect("validated window");
                Ok(Some(window.combo.set_item_data(args[2], args[3])))
            }
            0x14e => {
                let window = self.desktop.window_mut(args[0]).expect("validated window");
                let result = window.combo.select(args[2]);
                window.title = window.combo.selected_title().unwrap_or_default().to_owned();
                Ok(Some(result))
            }
            _ => Ok(None),
        }
    }

    pub(super) fn set_window_text(&mut self, args: &[u32]) -> Result<(), DispatchError> {
        if self.desktop.window(args[0]).is_none() {
            thread::Teb(self.cpu.fs_base()).set_last_error(&mut self.memory, 1400)?;
            self.cpu.set_register(Register32::Eax, 0);
            return Ok(());
        }
        let title = read_title(&self.memory, args[1])?;
        self.desktop
            .window_mut(args[0])
            .expect("validated window")
            .title = title;
        self.cpu.set_register(Register32::Eax, 1);
        Ok(())
    }

    pub(super) fn window_query(&mut self, call: Call, args: &[u32]) -> Result<(), DispatchError> {
        if matches!(call, Call::Top)
            && !matches!(args[0], 0 | DESKTOP)
            && self.desktop.window(args[0]).is_none()
        {
            thread::Teb(self.cpu.fs_base()).set_last_error(&mut self.memory, 1400)?;
            self.cpu.set_register(Register32::Eax, 0);
            return Ok(());
        }
        if matches!(call, Call::Window) {
            if args[1] > 5 {
                return Err(DispatchError::Unsupported);
            }
            if self.desktop.window(args[0]).is_none() {
                thread::Teb(self.cpu.fs_base()).set_last_error(&mut self.memory, 1400)?;
                self.cpu.set_register(Register32::Eax, 0);
                return Ok(());
            }
        }
        let value = self
            .desktop
            .dispatch(call, args, &self.user_atoms, &self.memory)?;
        self.cpu.set_register(super::Register32::Eax, value);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::Permissions;

    #[test]
    fn activation_epochs_change_only_with_the_active_window() {
        let mut desktop = Desktop::default();
        assert_eq!(desktop.activation(), (0, Some(0)));
        for handle in [4, 8] {
            desktop.insert(
                handle,
                Window {
                    style: 0x1000_0000,
                    ..Window::default()
                },
            );
        }
        desktop.insert(12, Window::default());
        desktop.activate_created(4);
        assert_eq!(desktop.activation(), (4, Some(1)));
        desktop.activate_created(4);
        assert!(desktop.activate_foreground(4));
        assert_eq!(desktop.show_activated(4), Some(1));
        assert_eq!(desktop.hide_window(8), Some(1));
        assert!(!desktop.activate_foreground(8));
        desktop.activate_created(12);
        desktop.remove(12);
        desktop.insert_child(
            16,
            Window {
                parent: 4,
                style: 0x5000_0000,
                ..Window::default()
            },
        );
        desktop.activate_created(16);
        assert!(!desktop.activate_foreground(16));
        desktop.remove_children(4);
        assert_eq!(desktop.activation(), (4, Some(1)));
        assert_eq!(desktop.show_activated(8), Some(0));
        assert_eq!(desktop.activation(), (8, Some(2)));
        assert!(desktop.activate_foreground(4));
        assert_eq!(desktop.activation(), (4, Some(3)));
        assert_eq!(desktop.hide_window(4), Some(1));
        assert_eq!(desktop.activation(), (0, Some(4)));
        assert_eq!(desktop.show_activated(4), Some(0));
        assert_eq!(desktop.activation(), (4, Some(5)));
        desktop.remove(8);
        assert_eq!(desktop.activation(), (4, Some(5)));
        desktop.remove(4);
        assert_eq!(desktop.activation(), (0, Some(6)));
    }

    #[test]
    fn activation_epoch_exhaustion_keeps_window_behavior_without_reusing_identity() {
        let mut desktop = Desktop {
            activation_epoch: Some(u64::MAX - 1),
            ..Desktop::default()
        };
        desktop.insert(
            4,
            Window {
                style: 0x1000_0000,
                ..Window::default()
            },
        );
        desktop.activate_created(4);
        assert_eq!(desktop.activation(), (4, Some(u64::MAX)));
        assert!(desktop.activate_foreground(4));
        assert_eq!(desktop.activation(), (4, Some(u64::MAX)));
        assert_eq!(desktop.hide_window(4), Some(1));
        assert_eq!(desktop.activation(), (0, None));
        assert_eq!(desktop.show_activated(4), Some(0));
        assert_eq!(desktop.activation(), (4, None));
        desktop.remove(4);
        assert_eq!(desktop.activation(), (0, None));
    }

    #[test]
    fn combo_items_keep_sorted_or_insertion_order_with_a_byte_limit() {
        let mut sorted = ComboBox::default();
        assert_eq!(sorted.add(0x100, "pear".into()), 0);
        assert_eq!(sorted.set_item_data(0, 0x1234), 0);
        assert_eq!(sorted.select(0), 0);
        assert_eq!(sorted.add(0x100, "Apple".into()), 0);
        assert_eq!(sorted.add(0x100, "orange".into()), 1);
        assert_eq!(sorted.add(0x100, "apple".into()), 1);
        assert_eq!(
            sorted
                .items
                .iter()
                .map(|item| item.title.as_str())
                .collect::<Vec<_>>(),
            ["Apple", "apple", "orange", "pear"]
        );
        assert_eq!(sorted.item_data(3), 0x1234);
        assert_eq!(sorted.selection(), 3);
        assert_eq!(sorted.selected_title(), Some("pear"));
        assert_eq!(sorted.select(u32::MAX), u32::MAX);
        assert_eq!(sorted.selection(), u32::MAX);
        assert_eq!(sorted.selected_title(), None);
        assert_eq!(sorted.set_item_data(4, 99), u32::MAX);
        assert_eq!(sorted.item_data(4), u32::MAX);

        let mut unsorted = ComboBox::default();
        assert_eq!(unsorted.add(0, "pear".into()), 0);
        assert_eq!(unsorted.add(0, "apple".into()), 1);
        assert_eq!(unsorted.items[0].title, "pear");
        assert_eq!(unsorted.items[1].title, "apple");

        let mut full = ComboBox::default();
        for index in 0..16 {
            assert_eq!(full.add(0, "x".repeat(4095)), index);
        }
        assert_eq!(full.add(0, "x".into()), u32::MAX - 1);
        assert_eq!(full.count(), 16);
        assert_eq!(full.select(0), 0);
        full.reset();
        assert_eq!(full.count(), 0);
        assert_eq!(full.selection(), u32::MAX);
        assert_eq!(full.item_data(0), u32::MAX);
        assert_eq!(full.add(0, "new".into()), 0);
    }

    #[test]
    fn snapshots_include_children_in_handle_order_without_desktop_sentinel() {
        let mut desktop = Desktop::default();
        desktop.top_levels.insert(
            0x7500_0008,
            Window {
                title: "dialog".into(),
                ..Window::default()
            },
        );
        desktop.children.insert(
            0x7500_0004,
            Window {
                parent: 0x7500_0008,
                id: 42,
                class: 0x80,
                dialog_units: Some([1, 2, 3, 4]),
                ..Window::default()
            },
        );
        let snapshots = desktop.snapshots();
        assert_eq!(desktop.top_window(0x7500_0008), 0x7500_0004);
        assert_eq!(desktop.top_window(0), 0x7500_0008);
        assert_eq!(desktop.top_window(0x7500_0004), 0);
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].hwnd, 0x7500_0004);
        assert_eq!(snapshots[0].parent, 0x7500_0008);
        assert_eq!(snapshots[0].id, 42);
        assert_eq!(snapshots[0].dialog_units, Some([1, 2, 3, 4]));
        assert_eq!(snapshots[1].hwnd, 0x7500_0008);
        desktop.children.clear();
        assert_eq!(snapshots[0].class, 0x80);
        assert_eq!(snapshots[1].title, "dialog");
    }

    #[test]
    fn end_dialog_records_result_hides_and_keeps_owned_handles() {
        let mut desktop = Desktop::default();
        desktop.insert(
            0x7500_0004,
            Window {
                class: 0x8002,
                style: 0x1000_0000,
                dialog_units: Some([0; 4]),
                needs_paint: true,
                ..Window::default()
            },
        );
        desktop.insert_child(
            0x7500_0008,
            Window {
                parent: 0x7500_0004,
                ..Window::default()
            },
        );
        desktop.activate_created(0x7500_0004);
        desktop.end_dialog(0x7500_0004, 123);
        let dialog = desktop.window(0x7500_0004).unwrap();
        assert_eq!(dialog.dialog_result, Some(123));
        assert_eq!(dialog.style & 0x1000_0000, 0);
        assert!(!dialog.needs_paint);
        assert_eq!(desktop.active, 0);
        assert!(desktop.window(0x7500_0008).is_some());
    }

    #[test]
    fn hiding_visible_dialog_twice_keeps_result_unset() {
        let mut desktop = Desktop::default();
        desktop.insert(
            0x7500_0004,
            Window {
                class: 0x8002,
                style: 0x1000_0000,
                dialog_units: Some([0; 4]),
                needs_paint: true,
                ..Window::default()
            },
        );
        desktop.activate_created(0x7500_0004);
        for _ in 0..2 {
            desktop.hide_dialog(0x7500_0004);
            let window = desktop.window(0x7500_0004).unwrap();
            assert_eq!(window.style & 0x1000_0000, 0);
            assert_eq!(window.dialog_result, None);
            assert!(!window.needs_paint);
            assert_eq!(desktop.active, 0);
        }
    }

    #[test]
    fn full_client_invalidation_is_owned_and_idempotent() {
        let mut desktop = Desktop::default();
        desktop.insert(
            0x7500_0004,
            Window {
                rectangle: [10, 20, 130, 90],
                client: [0, 0, 120, 70],
                ..Window::default()
            },
        );
        assert!(!desktop.window(0x7500_0004).unwrap().needs_paint);
        assert!(!desktop.invalidate_full(0));
        assert!(desktop.invalidate_full(0x7500_0004));
        assert!(desktop.invalidate_full(0x7500_0004));
        let window = desktop.window(0x7500_0004).unwrap();
        assert!(window.needs_paint);
        assert_eq!(window.rectangle, [10, 20, 130, 90]);
        assert_eq!(window.client, [0, 0, 120, 70]);
    }

    #[test]
    fn topmost_no_redraw_keeps_existing_paint_without_scheduling_another() {
        let mut desktop = Desktop::default();
        desktop.insert(
            0x7500_0004,
            Window {
                style: 0x1000_0000,
                rectangle: [0, 0, 640, 480],
                client: [3, 22, 637, 477],
                needs_paint: true,
                ..Window::default()
            },
        );
        desktop.activate_created(0x7500_0004);
        assert!(desktop.position_topmost_no_redraw(&[0x7500_0004, u32::MAX, 0, 0, 640, 461, 8]));
        let window = desktop.window(0x7500_0004).unwrap();
        assert_eq!(window.rectangle, [0, 0, 640, 461]);
        assert_eq!(window.client, [3, 22, 637, 458]);
        assert!(window.topmost);
        assert!(window.needs_paint);
        assert_eq!(desktop.active, 0x7500_0004);
    }

    #[test]
    fn foreground_activation_rejects_children_and_hidden_windows() {
        let mut desktop = Desktop::default();
        desktop.insert(
            0x7500_0004,
            Window {
                style: 0x1000_0000,
                ..Window::default()
            },
        );
        desktop.insert_child(
            0x7500_0008,
            Window {
                parent: 0x7500_0004,
                style: 0x1000_0000,
                ..Window::default()
            },
        );
        assert!(!desktop.activate_foreground(0x7500_0008));
        assert_eq!(desktop.active, 0);
        assert!(desktop.activate_foreground(0x7500_0004));
        assert_eq!(desktop.active, 0x7500_0004);
        desktop.hide_window(0x7500_0004);
        assert!(!desktop.activate_foreground(0x7500_0004));
        assert_eq!(desktop.active, 0);
    }

    #[test]
    fn populated_registry_filters_owned_class_and_title_and_excludes_desktop() {
        let mut memory = GuestMemory::new(1);
        memory
            .map_zeroed(0x10000, 4096, Permissions::READ_WRITE)
            .unwrap();
        memory.write(0x10000, b"DEMO\0").unwrap();
        memory.write(0x10020, b"TITLE\0").unwrap();
        memory.write(0x10040, b"unknown\0").unwrap();
        let mut atoms = UserAtoms::default();
        let Ok(atom) = atoms.retain_class("demo".into(), thread::Teb(thread::BASE), &mut memory)
        else {
            panic!("class atom allocation failed");
        };
        let mut desktop = Desktop::default();
        desktop.top_levels.insert(
            0x7500_0004,
            Window {
                class: atom,
                title: "title".into(),
                ..Window::default()
            },
        );
        desktop.top_levels.insert(
            0x7500_0008,
            Window {
                class: atom + 1,
                title: String::new(),
                ..Window::default()
            },
        );
        for (args, expected) in [
            ([0, 0], 0x7500_0008),
            ([0x10000, 0x10020], 0x7500_0004),
            ([atom, 0], 0x7500_0004),
            ([0, 0x10025], 0x7500_0008),
            ([atom + 1, 0x10020], 0),
            ([0x10040, 0], 0),
            ([0, 0x10040], 0),
        ] {
            assert!(
                matches!(desktop.dispatch(Call::Find, &args, &atoms, &memory), Ok(value) if value == expected)
            );
        }
        assert!(matches!(
            desktop.dispatch(Call::IsWindow, &[0x7500_0004], &atoms, &memory),
            Ok(1)
        ));
        desktop.top_levels.clear();
        assert!(matches!(
            desktop.dispatch(Call::Find, &[0, 0], &atoms, &memory),
            Ok(0)
        ));
        assert!(matches!(
            desktop.dispatch(Call::IsWindow, &[DESKTOP], &atoms, &memory),
            Ok(1)
        ));
        assert!(matches!(
            desktop.dispatch(Call::IsWindow, &[0x7500_0004], &atoms, &memory),
            Ok(0)
        ));
    }
}
