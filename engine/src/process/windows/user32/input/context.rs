use crate::{
    input::{
        keyboard::{self, KeyboardLayout32},
        source::{InputCursor32, SourceSnapshot32},
    },
    windows::user32::{Message32, Point32, Rect32},
};

#[derive(Clone, Copy)]
pub(in crate::process::windows::user32) struct User32Input32 {
    pub cursor: Option<InputCursor32>,
    pub asynchronous: [u8; 256],
    pub query_async: [u8; 256],
    pub query_recent: [bool; 256],
    pub observed_presses: [u64; 256],
    pub observed_buttons: [u64; 8],
    pub query_focus: bool,
    pub recent: [bool; 256],
    pub queued: [u8; 256],
    pub physical_focus: bool,
    pub physical_capture: bool,
    pub mouse_exclusive: bool,
    pub focus: u32,
    pub capture: u32,
    pub point: Point32,
    pub query_point: Point32,
    pub delivery_clip: Option<Rect32>,
    pub clip: Option<Rect32>,
    pub desktop: Rect32,
    pub cursor_request: Option<super::User32CursorRequest32>,
    pub cursor_revision: u64,
    pub layout: KeyboardLayout32,
    pub delivery_layout: KeyboardLayout32,
    pub delivery_swapped: bool,
    pub held_virtual_keys: [u8; 256],
    pub loaded: [bool; 2],
    pub dead: Option<char>,
    pub show_count: i32,
    pub double_click_ms: u32,
    pub swapped: bool,
    pub cursor_handle: u32,
    pub message: Message32,
    pub extra_info: u32,
    pub last_click: Option<(u32, u8, Point32, u32)>,
}

impl Default for User32Input32 {
    fn default() -> Self {
        Self {
            cursor: None,
            asynchronous: [0; 256],
            query_async: [0; 256],
            query_recent: [false; 256],
            observed_presses: [0; 256],
            observed_buttons: [0; 8],
            query_focus: false,
            recent: [false; 256],
            queued: [0; 256],
            physical_focus: false,
            physical_capture: false,
            mouse_exclusive: false,
            focus: 0,
            capture: 0,
            point: Point32::default(),
            query_point: Point32::default(),
            delivery_clip: None,
            clip: None,
            desktop: Rect32 {
                left: 0,
                top: 0,
                right: 640,
                bottom: 480,
            },
            cursor_request: None,
            cursor_revision: 0,
            layout: KeyboardLayout32::Us,
            delivery_layout: KeyboardLayout32::Us,
            delivery_swapped: false,
            held_virtual_keys: [0; 256],
            loaded: [true, false],
            dead: None,
            show_count: 0,
            double_click_ms: 500,
            swapped: false,
            cursor_handle: 0,
            message: Message32::default(),
            extra_info: 0,
            last_click: None,
        }
    }
}

impl User32Input32 {
    pub(in crate::process::windows::user32) fn attach(
        &mut self,
        cursor: InputCursor32,
        snapshot: &SourceSnapshot32,
    ) {
        self.cursor = Some(cursor);
        self.delivery_layout = self.layout;
        self.delivery_swapped = self.swapped;
        self.held_virtual_keys = [0; 256];
        self.physical_focus = snapshot.focused;
        self.physical_capture = snapshot.captured;
        self.mouse_exclusive = snapshot.mouse_exclusive;
        self.point = Point32 {
            x: snapshot.cursor.0,
            y: snapshot.cursor.1,
        };
        self.query_point = self.point;
        self.delivery_clip = snapshot
            .cursor_bounds
            .map(|(left, top, right, bottom)| Rect32 {
                left,
                top,
                right,
                bottom,
            });
        self.asynchronous = [0; 256];
        self.recent = [false; 256];
        for (scan, &down) in snapshot.keys.iter().enumerate() {
            let key = keyboard::virtual_key_for_layout(self.layout, scan as u16);
            if key != 0 && down {
                self.asynchronous[key as usize] = 0x80;
            }
        }
        for (button, &down) in snapshot.buttons.iter().enumerate() {
            if let Some(key) = mouse_key(button as u8) {
                self.asynchronous[key as usize] = u8::from(down) << 7;
            }
        }
        normalize_modifiers(&mut self.asynchronous);
        self.query_async = self.asynchronous;
        self.query_focus = snapshot.focused;
        self.observed_presses = snapshot.presses;
        self.observed_buttons = snapshot.button_presses;
    }

    pub(in crate::process::windows::user32) fn observe_async(
        &mut self,
        snapshot: &SourceSnapshot32,
    ) {
        self.query_focus = snapshot.focused;
        self.query_point = self.clip_point(Point32 {
            x: snapshot.cursor.0,
            y: snapshot.cursor.1,
        });
        self.query_async = [0; 256];
        if !snapshot.focused {
            self.query_recent = [false; 256];
        }
        for (scan, &pressed) in snapshot.keys.iter().enumerate() {
            let key = keyboard::virtual_key_for_layout(self.layout, scan as u16) as usize;
            if key != 0 {
                self.query_async[key] |= u8::from(pressed) << 7;
                let recent = snapshot.presses[scan] != self.observed_presses[scan];
                self.query_recent[key] |= recent && snapshot.focused;
                for (generic, left, right) in
                    [(0x10, 0xa0, 0xa1), (0x11, 0xa2, 0xa3), (0x12, 0xa4, 0xa5)]
                {
                    if key == left || key == right {
                        self.query_recent[generic] |= recent && snapshot.focused;
                    }
                }
            }
        }
        for (button, &pressed) in snapshot.buttons.iter().enumerate() {
            if let Some(key) = mouse_key(button as u8) {
                self.query_async[key as usize] = u8::from(pressed) << 7;
                self.query_recent[key as usize] |= snapshot.button_presses[button]
                    != self.observed_buttons[button]
                    && snapshot.focused;
            }
        }
        normalize_modifiers(&mut self.query_async);
        self.observed_presses = snapshot.presses;
        self.observed_buttons = snapshot.button_presses;
    }

    pub(super) fn asynchronous_key(&mut self, key: usize) -> u32 {
        if key > 255 || !self.query_focus {
            return 0;
        }
        let value =
            (u32::from(self.query_async[key] & 0x80) << 8) | u32::from(self.query_recent[key]);
        self.query_recent[key] = false;
        (value as i16 as i32) as u32
    }

    pub(super) fn clip_point(&self, point: Point32) -> Point32 {
        let clip = self.clip.unwrap_or(self.desktop);
        Point32 {
            x: point.x.clamp(clip.left, clip.right.saturating_sub(1)),
            y: point.y.clamp(clip.top, clip.bottom.saturating_sub(1)),
        }
    }
}

pub(super) fn normalize_modifiers(keys: &mut [u8; 256]) {
    for (generic, left, right) in [(0x10, 0xa0, 0xa1), (0x11, 0xa2, 0xa3), (0x12, 0xa4, 0xa5)] {
        keys[generic] = (keys[generic] & 1) | ((keys[left] | keys[right]) & 0x80);
    }
}

pub(super) fn mouse_key(button: u8) -> Option<u8> {
    match button {
        0 => Some(1),
        1 => Some(2),
        2 => Some(4),
        3 => Some(5),
        4 => Some(6),
        _ => None,
    }
}
