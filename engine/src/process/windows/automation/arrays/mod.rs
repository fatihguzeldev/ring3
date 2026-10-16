mod calls;
mod creation;
mod data;
#[cfg(test)]
mod string_tests;
mod strings;

use crate::windows::automation::ArrayLayout32;
pub(super) struct Array32 {
    pub base: u32,
    pub descriptor: u32,
    pub data: u32,
    pub allocation_bytes: usize,
    pub separate_data: bool,
    pub layout: ArrayLayout32,
}
