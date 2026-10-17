mod calls;
mod creation;
mod data;
mod loans;
mod owned;
pub(super) use loans::ArrayLoan32;
pub(in crate::process) use loans::PreparedAutomationArrays32;
#[cfg(test)]
mod reference_tests;
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
    pub borrows: u32,
    pub exclusive: bool,
    pub layout: ArrayLayout32,
}
