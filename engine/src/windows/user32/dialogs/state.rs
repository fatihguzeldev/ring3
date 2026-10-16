use super::super::Point32;

#[derive(Clone, Copy)]
pub(crate) struct WindowDialog32 {
    pub base: Point32,
    pub font: u32,
    pub modal: bool,
    pub ended: Option<u32>,
}
