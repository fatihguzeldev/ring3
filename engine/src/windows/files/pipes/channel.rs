use super::*;
use std::collections::VecDeque;

pub(super) struct Channel32 {
    pub bytes: Vec<u8>,
    pub head: usize,
    pub length: usize,
    pub messages: VecDeque<usize>,
}
impl Channel32 {
    pub(super) fn new(capacity: usize, messages: usize) -> Result<Self, FileError32> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(capacity)
            .map_err(|_| FileError32::Allocation)?;
        bytes.resize(capacity, 0);
        let mut frames = VecDeque::new();
        frames
            .try_reserve_exact(messages)
            .map_err(|_| FileError32::Allocation)?;
        Ok(Self {
            bytes,
            head: 0,
            length: 0,
            messages: frames,
        })
    }
    pub(super) fn copy(&self, length: usize) -> Result<Vec<u8>, FileError32> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| FileError32::Allocation)?;
        bytes.resize(length, 0);
        if length != 0 {
            let first = length.min(self.bytes.len() - self.head);
            bytes[..first].copy_from_slice(&self.bytes[self.head..self.head + first]);
            bytes[first..].copy_from_slice(&self.bytes[..length - first]);
        }
        Ok(bytes)
    }
    pub(super) fn append(&mut self, bytes: &[u8], message: bool) {
        assert!(self.length + bytes.len() <= self.bytes.len());
        if !bytes.is_empty() {
            let tail = (self.head + self.length) % self.bytes.len();
            let first = bytes.len().min(self.bytes.len() - tail);
            self.bytes[tail..tail + first].copy_from_slice(&bytes[..first]);
            self.bytes[..bytes.len() - first].copy_from_slice(&bytes[first..]);
        }
        self.length += bytes.len();
        if message {
            self.messages.push_back(bytes.len());
        }
    }
    pub(super) fn consume(&mut self, length: usize, message: bool) {
        assert!(length <= self.length);
        if !self.bytes.is_empty() {
            self.head = (self.head + length) % self.bytes.len();
        }
        self.length -= length;
        if message && length == 0 && self.messages.front() == Some(&0) {
            self.messages.pop_front();
            return;
        }
        let mut remaining = length;
        while let Some(front) = self.messages.front_mut() {
            if *front > remaining {
                *front -= remaining;
                break;
            }
            remaining -= *front;
            self.messages.pop_front();
            if remaining == 0 {
                break;
            }
        }
    }
    pub(super) fn clear(&mut self) {
        self.head = 0;
        self.length = 0;
        self.messages.clear();
    }
}
