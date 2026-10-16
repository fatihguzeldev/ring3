use super::{RegistryChangeId32, RegistryChangeKind32, RegistryKeyId32, RegistryView32};
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Hive32 {
    Machine,
    Users,
}
impl Hive32 {
    pub(super) fn handle(self) -> u32 {
        match self {
            Self::Machine => 0x8000_0002,
            Self::Users => 0x8000_0003,
        }
    }
}
pub(super) struct Value32 {
    pub name: Vec<u16>,
    pub ordinal: Vec<u16>,
    pub kind: u32,
    pub bytes: Vec<u8>,
}
pub(super) struct Key32 {
    pub generation: u64,
    pub hive: Hive32,
    pub view: RegistryView32,
    pub path: Vec<u16>,
    pub ordinal: Vec<u16>,
    pub class: Vec<u16>,
    pub modified: u64,
    pub values: Vec<Value32>,
    pub deleted: bool,
    pub handles: u32,
    pub volatile: bool,
}
impl Key32 {
    pub(super) fn bytes(&self) -> usize {
        (self.path.len() + self.ordinal.len() + self.class.len()) * 2
            + self
                .values
                .iter()
                .map(|value| (value.name.len() + value.ordinal.len()) * 2 + value.bytes.len())
                .sum::<usize>()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Target32 {
    pub primary: RegistryKeyId32,
    pub fallback: Option<RegistryKeyId32>,
    pub classes: bool,
}
pub(super) struct Handle32 {
    pub value: u32,
    pub target: Target32,
    pub access: u32,
    pub view: RegistryView32,
}
pub(super) struct Change32 {
    pub id: RegistryChangeId32,
    pub kind: RegistryChangeKind32,
    pub hive: Hive32,
    pub view: RegistryView32,
    pub path: Vec<u16>,
    pub name: Vec<u16>,
    pub key: Option<RegistryKeyId32>,
    pub modified: u64,
}
#[derive(Clone, Copy)]
pub struct RegistryValue32<'a> {
    pub kind: u32,
    pub bytes: &'a [u8],
}
pub struct RegistryKeyInfo32<'a> {
    pub class: &'a [u16],
    pub modified: u64,
    pub subkeys: u32,
    pub maximum_subkey_name: u32,
    pub maximum_subkey_class: u32,
    pub values: u32,
    pub maximum_value_name: u32,
    pub maximum_value_bytes: u32,
}
pub struct RegistryEntry32<'a> {
    pub name: &'a [u16],
    pub class: &'a [u16],
    pub modified: u64,
}
pub struct RegistryNamedValue32<'a> {
    pub name: &'a [u16],
    pub value: RegistryValue32<'a>,
}
pub struct RegistryChange32<'a> {
    pub id: RegistryChangeId32,
    pub kind: RegistryChangeKind32,
    pub root: u32,
    pub view: RegistryView32,
    pub path: &'a [u16],
    pub name: &'a [u16],
    pub class: &'a [u16],
    pub modified: u64,
    pub value: Option<RegistryValue32<'a>>,
}

impl Value32 {
    pub(super) fn bytes(&self) -> usize {
        (self.name.len() + self.ordinal.len()) * 2 + self.bytes.len()
    }
}
impl Change32 {
    pub(super) fn bytes(&self) -> usize {
        (self.path.len() + self.name.len()) * 2
    }
}
