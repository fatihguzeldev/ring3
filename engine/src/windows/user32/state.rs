use super::{Message32, Point32, User32Error32, User32Limits32, WindowClass32, WindowData32};
use crate::windows::window::{WindowContext32, WindowDescription32, WindowHandle32};
mod classes;
mod colors;
mod dialogs;
mod menus;
mod paint;
mod text;
mod ui;

struct WindowSlot32 {
    data: WindowData32,
    extra: Box<[u8]>,
    update: crate::windows::gdi::Region32,
    erase: bool,
    paint_seen: bool,
    text: Vec<u16>,
    control: crate::windows::controls::ControlState32,
    dialog: Option<super::dialogs::WindowDialog32>,
    context_help: u32,
    ui_state: u32,
}

pub(crate) struct User32State32 {
    limits: User32Limits32,
    classes: Vec<WindowClass32>,
    windows: WindowContext32,
    slots: Vec<WindowSlot32>,
    queue: super::queue::MessageQueue32,
    next_atom: u16,
    standard_atoms: [u16; 6],
    system_atoms: Vec<u16>,
}

impl User32State32 {
    pub(crate) fn new(limits: User32Limits32) -> Result<Self, User32Error32> {
        if !(1..=16_383).contains(&limits.classes) || !(1..=32).contains(&limits.continuations) {
            return Err(User32Error32::InvalidLimits);
        }
        let windows = WindowContext32::new(limits.windows, limits.messages)?;
        let mut classes = Vec::new();
        classes
            .try_reserve_exact(limits.classes)
            .map_err(|_| User32Error32::Allocation)?;
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(limits.windows)
            .map_err(|_| User32Error32::Allocation)?;
        let queue = super::queue::MessageQueue32::new(limits.messages)?;
        Ok(Self {
            limits,
            classes,
            windows,
            slots,
            queue,
            next_atom: 0xc000,
            standard_atoms: [0; 6],
            system_atoms: Vec::new(),
        })
    }

    pub(crate) fn windows(&self) -> &WindowContext32 {
        &self.windows
    }
    pub(crate) fn windows_mut(&mut self) -> &mut WindowContext32 {
        &mut self.windows
    }

    pub(crate) fn register_class(
        &mut self,
        mut class: WindowClass32,
    ) -> Result<u16, User32Error32> {
        if class.name.is_empty()
            || class.name.len() > 255
            || class.procedure.0 == 0
            || class.extra_bytes > 4096
        {
            return Err(User32Error32::Win32(87));
        }
        if self.classes.iter().any(|existing| {
            existing.instance == class.instance && names_equal(&existing.name, &class.name)
        }) {
            return Err(User32Error32::Win32(1410));
        }
        if self
            .classes
            .iter()
            .filter(|class| !self.is_system_class(class.atom))
            .count()
            == self.limits.classes
        {
            return Err(User32Error32::Capacity);
        }
        let next = self
            .next_atom
            .checked_add(1)
            .ok_or(User32Error32::Capacity)?;
        class.atom = next;
        self.classes.push(class);
        self.next_atom = next;
        Ok(next)
    }

    pub(crate) fn class(
        &self,
        instance: u32,
        name: &[u16],
        atom: Option<u16>,
    ) -> Result<&WindowClass32, User32Error32> {
        self.classes
            .iter()
            .filter(|class| class.instance == instance)
            .chain(
                self.classes
                    .iter()
                    .filter(|class| class.instance != instance && class.style & 0x4000 != 0),
            )
            .find(|class| {
                (class.instance == instance || class.style & 0x4000 != 0)
                    && atom
                        .map_or_else(|| names_equal(&class.name, name), |atom| class.atom == atom)
            })
            .ok_or(User32Error32::Win32(1407))
    }

    pub(crate) fn unregister_class(
        &mut self,
        instance: u32,
        name: &[u16],
        atom: Option<u16>,
    ) -> Result<(), User32Error32> {
        let index = self
            .classes
            .iter()
            .position(|class| {
                class.instance == instance
                    && atom
                        .map_or_else(|| names_equal(&class.name, name), |atom| class.atom == atom)
            })
            .ok_or(User32Error32::Win32(1411))?;
        if self.is_system_class(self.classes[index].atom) {
            return Err(User32Error32::Win32(1411));
        }
        if self
            .slots
            .iter()
            .any(|slot| slot.data.class_atom == self.classes[index].atom)
        {
            return Err(User32Error32::Win32(1412));
        }
        self.classes.remove(index);
        Ok(())
    }

    pub(crate) fn check_create(
        &self,
        description: WindowDescription32,
    ) -> Result<WindowHandle32, User32Error32> {
        self.queue.check_external_change()?;
        Ok(self.windows.check_create(description)?)
    }

    pub(crate) fn publish_window(
        &mut self,
        description: WindowDescription32,
        mut data: WindowData32,
        extra: Box<[u8]>,
    ) -> WindowHandle32 {
        let handle = self
            .windows
            .create(description)
            .expect("preflighted window publication");
        data.handle = handle;
        self.slots.push(WindowSlot32 {
            data,
            extra,
            text: Vec::new(),
            control: Default::default(),
            dialog: None,
            context_help: 0,
            ui_state: 0,
            update: crate::windows::gdi::Region32::rectangle(super::Rect32 {
                left: 0,
                top: 0,
                right: description.extent.width as i32,
                bottom: description.extent.height as i32,
            }),
            erase: true,
            paint_seen: false,
        });
        self.queue.commit_external_change();
        handle
    }

    pub(crate) fn top_level(
        &self,
        mut handle: WindowHandle32,
    ) -> Result<WindowHandle32, User32Error32> {
        for _ in 0..=self.slots.len() {
            let data = self.record(handle)?;
            if data.style & 0x4000_0000 == 0 {
                return Ok(handle);
            }
            handle = data.parent.ok_or(User32Error32::Win32(1400))?;
        }
        Err(User32Error32::InvalidRequest)
    }

    pub(crate) fn related_windows(
        &self,
        parent: WindowHandle32,
        child: bool,
    ) -> impl DoubleEndedIterator<Item = WindowHandle32> + '_ {
        self.slots
            .iter()
            .filter(move |slot| {
                slot.data.parent == Some(parent) && (slot.data.style & 0x4000_0000 != 0) == child
            })
            .map(|slot| slot.data.handle)
    }

    pub(crate) fn destroy_tree_contains(
        &self,
        mut window: WindowHandle32,
        root: WindowHandle32,
    ) -> bool {
        for _ in 0..=self.slots.len() {
            if window == root {
                return true;
            }
            let Ok(data) = self.record(window) else {
                return false;
            };
            let Some(parent) = data.parent else {
                return false;
            };
            window = parent;
        }
        false
    }

    pub(crate) fn record(&self, handle: WindowHandle32) -> Result<WindowData32, User32Error32> {
        Ok(self.slots[self.slot(handle)?].data)
    }

    pub(crate) fn update(&mut self, data: WindowData32) -> Result<(), User32Error32> {
        let index = self.slot(data.handle)?;
        self.slots[index].data = data;
        Ok(())
    }

    pub(crate) fn retire_thread(&mut self, thread: u32) {
        self.slots.retain(|slot| {
            self.windows
                .get(slot.data.handle)
                .is_ok_and(|record| record.description.thread_id != thread)
        });
        self.windows.retire_thread(thread);
        self.queue.retire_thread(thread);
    }
    pub(crate) fn discard_raw_input(&mut self, thread: u32, ids: &[u32]) {
        self.queue.discard_raw_input(thread, ids);
    }

    pub(crate) fn remove_delivered(
        &mut self,
        thread_id: u32,
        handle: WindowHandle32,
    ) -> Result<(), User32Error32> {
        let index = self.slot(handle)?;
        self.windows.remove_delivered(thread_id, handle)?;
        self.slots.remove(index);
        self.queue.retain_window(handle.get());
        Ok(())
    }

    pub(crate) fn check_move_children(
        &self,
        handle: WindowHandle32,
        origin: Point32,
    ) -> Result<(i64, i64), User32Error32> {
        let old = self.record(handle)?.client_origin;
        let delta = (
            i64::from(origin.x) - i64::from(old.x),
            i64::from(origin.y) - i64::from(old.y),
        );
        for slot in &self.slots {
            if slot.data.handle != handle
                && self.is_descendant(slot.data.handle.get(), handle.get())
            {
                shifted_data(slot.data, delta)?;
            }
        }
        Ok(delta)
    }

    pub(crate) fn move_children(&mut self, handle: WindowHandle32, delta: (i64, i64)) {
        for index in 0..self.slots.len() {
            let data = self.slots[index].data;
            if data.handle != handle && self.is_descendant(data.handle.get(), handle.get()) {
                self.slots[index].data =
                    shifted_data(data, delta).expect("child coordinates were preflighted");
            }
        }
    }

    pub(crate) fn client_position(&self, handle: WindowHandle32) -> Result<Point32, User32Error32> {
        let data = self.record(handle)?;
        if data.style & 0x4000_0000 == 0 {
            return Ok(data.client_origin);
        }
        let parent = self
            .record(data.parent.ok_or(User32Error32::Win32(1400))?)?
            .client_origin;
        Ok(Point32 {
            x: data
                .client_origin
                .x
                .checked_sub(parent.x)
                .ok_or(User32Error32::Win32(87))?,
            y: data
                .client_origin
                .y
                .checked_sub(parent.y)
                .ok_or(User32Error32::Win32(87))?,
        })
    }

    pub(crate) fn extra(&self, handle: WindowHandle32, offset: u32) -> Result<u32, User32Error32> {
        let bytes = self.slots[self.slot(handle)?]
            .extra
            .get(offset as usize..offset as usize + 4)
            .ok_or(User32Error32::Win32(1413))?;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }

    pub(crate) fn set_extra(
        &mut self,
        handle: WindowHandle32,
        offset: u32,
        value: u32,
    ) -> Result<u32, User32Error32> {
        let old = self.extra(handle, offset)?;
        let index = self.slot(handle)?;
        self.slots[index].extra[offset as usize..offset as usize + 4]
            .copy_from_slice(&value.to_le_bytes());
        Ok(old)
    }

    pub(crate) fn post(&mut self, message: Message32) -> Result<(), User32Error32> {
        self.queue.post(message)
    }
    pub(crate) fn post_captured(
        &mut self,
        message: Message32,
        activation: crate::windows::activation::CapturedActivation32,
    ) -> Result<(), User32Error32> {
        self.queue.post_captured(message, activation)
    }
    pub(crate) fn message_activation(
        &self,
        index: usize,
    ) -> (u64, crate::windows::activation::CapturedActivation32) {
        self.queue.activation(index)
    }
    pub(crate) fn project_input_messages(
        &self,
        thread: u32,
        messages: &[Message32],
    ) -> Result<(u32, u32, u64), User32Error32> {
        self.queue
            .project_input(thread, messages)
            .map(|status| self.with_paint_status(thread, status))
    }
    pub(crate) fn post_input(
        &mut self,
        message: Message32,
        key: Option<super::QueuedKey32>,
        state: [u8; 256],
    ) -> Result<(), User32Error32> {
        self.queue.post_input(message, key, state)
    }
    pub(crate) fn post_raw_input(&mut self, message: Message32) -> Result<(), User32Error32> {
        self.queue.post_raw_input(message)
    }
    pub(crate) fn can_post(&self, count: usize) -> Result<(), User32Error32> {
        self.queue.can_post(count)
    }
    pub(crate) fn queue_status(&self, thread: u32) -> (u32, u32, u64) {
        self.with_paint_status(thread, self.queue.status(thread))
    }
    pub(crate) fn prepare_sent_change(
        &self,
        thread: u32,
        delta: i32,
    ) -> Result<super::PreparedSentChange32, User32Error32> {
        let paint = self.paint_status(thread);
        self.queue
            .prepare_sent_change(thread, delta)
            .map(|prepared| prepared.with_flags(paint.0, paint.1))
    }
    pub(crate) fn commit_sent_change(&mut self, prepared: super::PreparedSentChange32) {
        self.queue.commit_sent_change(prepared);
    }
    pub(crate) fn observe_queue(&mut self, thread: u32, mask: u32) {
        if mask & 0x20 != 0 {
            for slot in &mut self.slots {
                if self
                    .windows
                    .get(slot.data.handle)
                    .is_ok_and(|window| window.description.thread_id == thread)
                {
                    slot.paint_seen = true;
                }
            }
        }
        self.queue.observe(thread, mask)
    }
    pub(crate) fn post_quit(&mut self, message: Message32) -> Result<(), User32Error32> {
        self.queue.post_quit(message)
    }

    pub(crate) fn find_message(
        &self,
        thread_id: u32,
        window: u32,
        min: u32,
        max: u32,
    ) -> Result<Option<(usize, Message32)>, User32Error32> {
        self.find_message_mask(thread_id, window, min, max, u32::MAX)
    }
    pub(crate) fn find_message_mask(
        &self,
        thread_id: u32,
        window: u32,
        min: u32,
        max: u32,
        mask: u32,
    ) -> Result<Option<(usize, Message32)>, User32Error32> {
        if window != 0
            && window != u32::MAX
            && self
                .windows
                .get(WindowHandle32::try_from(window)?)?
                .description
                .thread_id
                != thread_id
        {
            return Err(User32Error32::Win32(1400));
        }
        Ok(self.queue.find(thread_id, mask, |message| {
            message.message == 0x12
                || ((window == 0
                    || window == u32::MAX && message.window == 0
                    || self.is_descendant(message.window, window))
                    && (min == 0 && max == 0 || (min..=max).contains(&message.message)))
        }))
    }

    pub(crate) fn discard_input_messages(&mut self) {
        self.queue.discard_input();
    }
    pub(crate) fn queue_without_input(&self, thread: u32) -> (u32, u32, u64) {
        self.with_paint_status(thread, self.queue.project_without_input(thread))
    }
    pub(crate) fn remove_message(&mut self, index: usize) -> Option<super::QueuedKeyboard32> {
        self.queue.remove(index)
    }

    fn is_descendant(&self, mut window: u32, parent: u32) -> bool {
        for _ in 0..self.slots.len() + 1 {
            if window == parent {
                return true;
            }
            let Some(next) = self
                .slots
                .iter()
                .find(|slot| slot.data.handle.get() == window && slot.data.style & 0x4000_0000 != 0)
                .and_then(|slot| slot.data.parent)
            else {
                return false;
            };
            window = next.get();
        }
        false
    }

    fn slot(&self, handle: WindowHandle32) -> Result<usize, User32Error32> {
        self.slots
            .binary_search_by_key(&handle, |slot| slot.data.handle)
            .map_err(|_| User32Error32::Win32(1400))
    }
}

fn names_equal(left: &[u16], right: &[u16]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(&left, &right)| {
            fn upper(unit: u16) -> u16 {
                let Some(value) = char::from_u32(u32::from(unit)) else {
                    return unit;
                };
                let mut mapped = value.to_uppercase();
                let first = mapped.next().unwrap() as u32;
                if mapped.next().is_none() && first <= u32::from(u16::MAX) {
                    first as u16
                } else {
                    unit
                }
            }
            upper(left) == upper(right)
        })
}

fn shifted_data(mut data: WindowData32, delta: (i64, i64)) -> Result<WindowData32, User32Error32> {
    let shift = |value: i32, delta: i64| {
        i32::try_from(i64::from(value) + delta).map_err(|_| User32Error32::Win32(87))
    };
    data.outer.left = shift(data.outer.left, delta.0)?;
    data.outer.right = shift(data.outer.right, delta.0)?;
    data.outer.top = shift(data.outer.top, delta.1)?;
    data.outer.bottom = shift(data.outer.bottom, delta.1)?;
    data.client_origin.x = shift(data.client_origin.x, delta.0)?;
    data.client_origin.y = shift(data.client_origin.y, delta.1)?;
    Ok(data)
}
