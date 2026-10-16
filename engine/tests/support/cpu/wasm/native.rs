#![allow(dead_code)]
#[path = "interface.rs"]
mod interface;
#[path = "reader.rs"]
mod reader;
#[path = "validation.rs"]
mod validation;

pub fn assert_bound(
    wasm: &[u8],
    resident: bool,
    helpers: &[(&str, usize)],
    arities: &[usize],
    segmented: bool,
) {
    interface::assert_bound(wasm, resident, helpers, arities, segmented);
}
pub fn assert_interface(
    wasm: &[u8],
    expected: &[(&str, usize)],
    arities: &[usize],
    segmented: bool,
) {
    interface::assert_interface(wasm, expected, arities, segmented);
}
pub fn assert_binding_prefix(wasm: &[u8], key: u64, id: u64, resident: bool) {
    interface::assert_binding_prefix(wasm, key, id, resident);
}
pub fn assert_packed(input: &[u8], count: usize, profiled: bool) {
    interface::assert_packed(input, count, profiled);
}
