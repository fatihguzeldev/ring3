use std::collections::VecDeque;

use super::desktop::Desktop;

const LIMIT: usize = 1024;
const WM_QUIT: u32 = 0x12;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PostedMessage {
    pub hwnd: u32,
    pub message: u32,
    pub wparam: u32,
    pub lparam: u32,
    pub time: u32,
    pub point: [i32; 2],
}

impl PostedMessage {
    pub(super) fn bytes(self) -> [u8; 32] {
        let mut bytes = [0; 32];
        for (index, word) in [
            self.hwnd,
            self.message,
            self.wparam,
            self.lparam,
            self.time,
            self.point[0].cast_unsigned(),
            self.point[1].cast_unsigned(),
            0,
        ]
        .iter()
        .enumerate()
        {
            bytes[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        bytes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PostMessageError {
    InvalidWindow,
    InvalidMessage,
    Full,
    Exited,
}

#[derive(Default)]
pub(super) struct Queue {
    entries: VecDeque<PostedMessage>,
}

impl Queue {
    pub(super) fn post(&mut self, message: PostedMessage) -> Result<(), PostMessageError> {
        if self.entries.len() == LIMIT {
            return Err(PostMessageError::Full);
        }
        self.entries.push_back(message);
        Ok(())
    }

    pub(super) fn find(
        &self,
        hwnd: u32,
        minimum: u32,
        maximum: u32,
        desktop: &Desktop,
    ) -> Option<(usize, PostedMessage)> {
        self.entries
            .iter()
            .enumerate()
            .find(|(_, message)| {
                message.message != WM_QUIT
                    && window_matches(**message, hwnd, desktop)
                    && ((minimum == 0 && maximum == 0)
                        || (minimum <= message.message && message.message <= maximum))
            })
            .or_else(|| {
                self.entries
                    .iter()
                    .enumerate()
                    .find(|(_, message)| message.message == WM_QUIT)
            })
            .map(|(index, message)| (index, *message))
    }

    pub(super) fn discard_retired_windows(&mut self, desktop: &Desktop) {
        self.entries
            .retain(|message| message.hwnd == 0 || desktop.window(message.hwnd).is_some());
    }

    pub(super) fn remove(&mut self, index: usize) {
        self.entries
            .remove(index)
            .expect("selected queue entry exists");
    }
}

fn window_matches(message: PostedMessage, filter: u32, desktop: &Desktop) -> bool {
    if filter == 0 {
        return true;
    }
    if filter == u32::MAX {
        return message.hwnd == 0;
    }
    let mut hwnd = message.hwnd;
    for _ in 0..16 {
        if hwnd == filter {
            return true;
        }
        let Some(window) = desktop.window(hwnd) else {
            break;
        };
        if window.parent == 0 {
            break;
        }
        hwnd = window.parent;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{Desktop, PostedMessage, Queue};
    use crate::execution::windows::desktop::Window;

    #[test]
    fn parent_window_filter_includes_owned_children() {
        let mut desktop = Desktop::default();
        let parent = desktop.available().unwrap();
        desktop.insert(parent, Window::default());
        let child = desktop.available().unwrap();
        desktop.insert_child(
            child,
            Window {
                parent,
                ..Window::default()
            },
        );
        let mut queue = Queue::default();
        queue
            .post(PostedMessage {
                hwnd: child,
                message: 0x401,
                wparam: 0,
                lparam: 0,
                time: 0,
                point: [0, 0],
            })
            .unwrap();
        assert_eq!(queue.find(parent, 0, 0, &desktop).unwrap().1.hwnd, child);
        assert!(queue.find(u32::MAX, 0, 0, &desktop).is_none());
    }
}
