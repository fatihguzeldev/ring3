use crate::windows::user32::{Point32, Rect32, User32Error32 as E};
pub(crate) const MAX_TOOLS: usize = 1024;
pub(crate) const MAX_TEXT: usize = 32767;
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Tool32 {
    pub key: u64,
    pub window: u32,
    pub id: u32,
    pub flags: u32,
    pub rect: Rect32,
    pub data: u32,
    pub text: Vec<u16>,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Tooltip32 {
    pub tools: Vec<Tool32>,
    pub active: Option<u64>,
    pub hover: Option<u64>,
    pub automatic: bool,
    pub suppressed: bool,
    pub enabled: bool,
    pub point: Point32,
    pub font: u32,
    pub foreground: u32,
    pub background: u32,
    pub unicode: bool,
    pub margin: Rect32,
    pub delays: [u32; 3],
    next_key: u64,
}
impl Default for Tooltip32 {
    fn default() -> Self {
        Self {
            tools: Vec::new(),
            active: None,
            hover: None,
            automatic: false,
            suppressed: false,
            enabled: true,
            point: Point32::default(),
            font: 0,
            foreground: 0xff000000,
            background: 0xff000000,
            unicode: true,
            margin: Rect32 {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            },
            delays: [500, 5000, 100],
            next_key: 1,
        }
    }
}
impl Tooltip32 {
    pub(crate) fn find(&self, window: u32, id: u32) -> Option<usize> {
        self.tools
            .iter()
            .position(|tool| tool.window == window && tool.id == id)
    }
    pub(crate) fn current(&self) -> Option<&Tool32> {
        self.tools.iter().find(|tool| Some(tool.key) == self.active)
    }
    pub(crate) fn prepare(&mut self, tool: &Tool32, replaced: Option<usize>) -> Result<(), E> {
        if tool.text.len() > MAX_TEXT {
            return Err(E::Capacity);
        }
        let total = self.tools.iter().map(|tool| tool.text.len()).sum::<usize>();
        let old = replaced.map_or(0, |index| self.tools[index].text.len());
        if total - old + tool.text.len() > 524288 {
            return Err(E::Capacity);
        }
        if replaced.is_none() {
            if self.tools.len() == MAX_TOOLS {
                return Err(E::Capacity);
            }
            self.next_key.checked_add(1).ok_or(E::Capacity)?;
            self.tools.try_reserve(1).map_err(|_| E::Allocation)?;
        }
        Ok(())
    }
    pub(crate) fn insert(&mut self, mut tool: Tool32) {
        tool.key = self.next_key;
        self.next_key += 1;
        self.tools.push(tool);
    }
    pub(crate) fn remove(&mut self, index: usize) {
        let tool = self.tools.remove(index);
        if self.active == Some(tool.key) {
            self.active = None;
            self.automatic = false;
        }
        if self.hover == Some(tool.key) {
            self.hover = None;
            self.suppressed = false;
        }
    }
}
#[cfg(test)]
mod tests;
