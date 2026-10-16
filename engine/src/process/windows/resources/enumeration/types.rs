use crate::{
    loader::{ModuleId32, ResourceIdentifier32},
    memory::GuestAddress,
    process::windows::callbacks::GuestCallbackScratch32,
};
#[derive(Clone, Copy)]
pub(in crate::process::windows::resources) enum Enumeration32 {
    Types,
    Names,
    Languages,
}
pub(in crate::process::windows::resources) enum EnumerationItem32 {
    Identifier(ResourceIdentifier32),
    Language(u16),
}
pub(in crate::process::windows::resources) struct PendingEnum32 {
    pub continuation: u64,
    pub thread: u32,
    pub module: ModuleId32,
    pub base: u32,
    pub kind: Enumeration32,
    pub resource_type: Option<ResourceIdentifier32>,
    pub resource_name: Option<ResourceIdentifier32>,
    pub items: Vec<EnumerationItem32>,
    pub index: usize,
    pub wide: bool,
    pub procedure: GuestAddress,
    pub parameter: u32,
    pub scratch: Option<GuestCallbackScratch32>,
    pub charged: usize,
}
impl PendingEnum32 {
    pub(super) fn metadata_bytes(&self) -> usize {
        fn bytes(id: &ResourceIdentifier32) -> usize {
            match id {
                ResourceIdentifier32::Id(_) => 0,
                ResourceIdentifier32::Name(name) => name.len() * 2,
            }
        }
        self.items.len() * std::mem::size_of::<EnumerationItem32>()
            + self
                .items
                .iter()
                .map(|item| match item {
                    EnumerationItem32::Identifier(id) => bytes(id),
                    EnumerationItem32::Language(_) => 0,
                })
                .sum::<usize>()
            + self.resource_type.as_ref().map_or(0, bytes)
            + self.resource_name.as_ref().map_or(0, bytes)
    }
}
