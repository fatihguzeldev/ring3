use crate::windows::user32::{Rect32, User32Error32 as E};

pub(crate) const MAX_PARTS: usize = 256;
pub(crate) const MAX_TEXT: usize = 65_535;
const MAX_TOTAL_TEXT: usize = 1_048_576;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct StatusPart32 {
    pub right: i32,
    pub kind: u32,
    pub data: u32,
    pub icon: u32,
    pub text: Vec<u16>,
}
impl Default for StatusPart32 {
    fn default() -> Self {
        Self {
            right: -1,
            kind: 0,
            data: 0,
            icon: 0,
            text: Vec::new(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Status32 {
    first: StatusPart32,
    parts: Vec<StatusPart32>,
    simple: StatusPart32,
    pub simple_mode: bool,
    pub minimum_height: i32,
    pub font: u32,
    pub background: u32,
    pub unicode: bool,
}
impl Default for Status32 {
    fn default() -> Self {
        Self {
            first: Default::default(),
            parts: Vec::new(),
            simple: Default::default(),
            simple_mode: false,
            minimum_height: 0,
            font: 0,
            background: 0xff00_0000,
            unicode: true,
        }
    }
}

pub(crate) struct PreparedStatusParts32 {
    edges: Vec<i32>,
    parts: Vec<StatusPart32>,
}

impl Status32 {
    pub(crate) fn count(&self) -> usize {
        self.parts.len() + 1
    }

    pub(crate) fn part(&self, index: usize) -> Option<&StatusPart32> {
        match index {
            0 => Some(&self.first),
            _ => self.parts.get(index - 1),
        }
    }

    pub(crate) fn part_mut(&mut self, index: usize) -> Option<&mut StatusPart32> {
        match index {
            0 => Some(&mut self.first),
            _ => self.parts.get_mut(index - 1),
        }
    }

    pub(crate) fn text_part(&self, index: usize) -> Option<&StatusPart32> {
        if index == 0xff {
            Some(&self.simple)
        } else {
            self.part(index)
        }
    }

    pub(crate) fn text_part_mut(&mut self, index: usize) -> Option<&mut StatusPart32> {
        if index == 0xff {
            Some(&mut self.simple)
        } else {
            self.part_mut(index)
        }
    }

    pub(crate) fn prepare_parts(edges: Vec<i32>) -> Result<PreparedStatusParts32, E> {
        if edges.is_empty() || edges.len() > MAX_PARTS {
            return Err(E::Win32(87));
        }
        let mut parts = Vec::new();
        parts
            .try_reserve_exact(edges.len() - 1)
            .map_err(|_| E::Allocation)?;
        Ok(PreparedStatusParts32 { edges, parts })
    }

    pub(crate) fn commit_parts(&mut self, mut prepared: PreparedStatusParts32) {
        self.first.right = prepared.edges[0];
        let mut previous = std::mem::take(&mut self.parts).into_iter();
        for right in prepared.edges.into_iter().skip(1) {
            let mut part = previous.next().unwrap_or_default();
            part.right = right;
            prepared.parts.push(part);
        }
        self.parts = prepared.parts;
    }

    pub(crate) fn check_text(&self, index: usize, length: usize) -> Result<(), E> {
        let old = self.text_part(index).ok_or(E::Win32(87))?;
        let total = self.first.text.len()
            + self.simple.text.len()
            + self.parts.iter().map(|part| part.text.len()).sum::<usize>();
        if length > MAX_TEXT || total - old.text.len() + length > MAX_TOTAL_TEXT {
            return Err(E::Capacity);
        }
        Ok(())
    }

    pub(crate) fn rect(&self, index: usize, width: i32, height: i32) -> Option<Rect32> {
        if index >= self.count() {
            return None;
        }
        let left = if index == 0 {
            0
        } else {
            let right = self.part(index - 1)?.right;
            if right == -1 { width } else { right }
        };
        let right = self.part(index)?.right;
        Some(Rect32 {
            left: left.max(0).min(width),
            top: 2.min(height),
            right: if right == -1 {
                width
            } else {
                right.max(0).min(width)
            },
            bottom: height,
        })
    }
}

#[cfg(test)]
mod tests;
