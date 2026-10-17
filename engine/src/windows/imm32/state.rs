use super::ImmError32;
use crate::windows::{
    gdi::LogicalFont32,
    user32::{Point32, Rect32},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompositionWindow32 {
    pub style: u32,
    pub position: Point32,
    pub area: Rect32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CandidateWindow32 {
    pub index: u32,
    pub style: u32,
    pub position: Point32,
    pub area: Rect32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputComposition32 {
    pub text: Vec<u16>,
    pub result: Vec<u16>,
    pub attributes: Vec<u8>,
    pub clauses: Vec<u32>,
    pub cursor: u32,
}
impl InputComposition32 {
    pub fn check(&self, unit_limit: usize) -> Result<(), ImmError32> {
        if self.text.len() > unit_limit || self.result.len() > unit_limit {
            return Err(ImmError32::Capacity);
        }
        if self.cursor as usize > self.text.len()
            || self.clauses.len() > self.text.len() + 1
            || !self.attributes.is_empty() && self.attributes.len() != self.text.len()
            || !self.clauses.is_empty()
                && (self.clauses.first() != Some(&0)
                    || self.clauses.last().copied().map(|v| v as usize) != Some(self.text.len())
                    || self.clauses.windows(2).any(|pair| pair[0] > pair[1]))
        {
            return Err(ImmError32::InvalidParameter);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputContext32 {
    pub(super) handle: u32,
    pub(super) thread: u32,
    pub(super) default: bool,
    pub open: bool,
    pub conversion: u32,
    pub sentence: u32,
    pub composition_window: Option<CompositionWindow32>,
    pub candidate_windows: [Option<CandidateWindow32>; 4],
    pub font: LogicalFont32,
    pub composition: InputComposition32,
}
impl InputContext32 {
    pub fn handle(&self) -> u32 {
        self.handle
    }
    pub fn thread(&self) -> u32 {
        self.thread
    }
    pub(super) fn new(handle: u32, thread: u32, default: bool) -> Self {
        Self {
            handle,
            thread,
            default,
            open: false,
            conversion: 1,
            sentence: 8,
            composition_window: None,
            candidate_windows: [None; 4],
            font: LogicalFont32::stock(13),
            composition: InputComposition32::default(),
        }
    }
}
