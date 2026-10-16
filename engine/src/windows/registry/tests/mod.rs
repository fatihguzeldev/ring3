mod compound;
mod identity;
mod notifications;
mod values;
use super::*;
fn units(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}
fn context(separate_views: bool) -> Registry32 {
    Registry32::new(
        7,
        RegistryConfig32 {
            separate_views,
            ..Default::default()
        },
    )
    .unwrap()
}
fn create(registry: &mut Registry32, root: u32, path: &str, access: u32) -> u32 {
    let prepared = registry
        .prepare_create(root, &units(path), access, &[], false, 100, true)
        .unwrap();
    registry.commit(prepared)
}
fn set(registry: &mut Registry32, handle: u32, name: &str, data: &[u8]) {
    let prepared = registry
        .prepare_set_value(handle, &units(name), 3, data, 200, true)
        .unwrap();
    registry.commit(prepared);
}
