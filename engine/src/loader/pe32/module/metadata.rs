use crate::memory::{GuestAddress, Permissions};
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParserLimits32 {
    pub metadata_entries: usize,
    pub string_bytes: usize,
}

impl Default for ParserLimits32 {
    fn default() -> Self {
        Self {
            metadata_entries: 65_536,
            string_bytes: 8 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageKind32 {
    Exe,
    Dll,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportSymbol32 {
    Name(String),
    Ordinal(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Directory32 {
    pub rva: u32,
    pub size: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaticTls32 {
    pub template_rva: u32,
    pub template_size: u32,
    pub zero_fill: u32,
    pub index_rva: u32,
    pub callbacks: Vec<u32>,
    pub callbacks_rva: Option<u32>,
    pub alignment: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadConfig32 {
    pub directory: Directory32,
    pub declared_size: u32,
    pub security_cookie_rva: Option<u32>,
    pub seh_handlers: Vec<u32>,
    pub guard_flags: Option<u32>,
}

#[derive(Debug)]
pub struct PreparedImagePage32 {
    pub address: GuestAddress,
    pub permissions: Permissions,
    pub bytes: Box<[u8; 4096]>,
}

#[derive(Clone, Debug)]
pub(crate) struct Section32 {
    pub rva: u32,
    pub virtual_size: u32,
    pub raw: Range<usize>,
    pub permissions: Permissions,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImportSlot32 {
    pub iat_rva: u32,
    pub symbol: ImportSymbol32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImportModule32 {
    pub name: String,
    pub slots: Vec<ImportSlot32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ExportTarget32 {
    Address(u32),
    Forwarder(String),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Exports32 {
    pub ordinal_base: u32,
    pub targets: Vec<Option<ExportTarget32>>,
    pub names: Vec<(String, u32)>,
}

impl Exports32 {
    pub(crate) fn lookup(&self, symbol: &ImportSymbol32) -> Option<&ExportTarget32> {
        let index = match symbol {
            ImportSymbol32::Ordinal(ordinal) => {
                u32::from(*ordinal).checked_sub(self.ordinal_base)?
            }
            ImportSymbol32::Name(name) => {
                let index = self
                    .names
                    .binary_search_by(|(candidate, _)| candidate.cmp(name))
                    .ok()?;
                self.names[index].1
            }
        };
        self.targets.get(index as usize)?.as_ref()
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Relocation32 {
    High(u32),
    Low(u32),
    HighLow(u32),
    HighAdj(u32, i16),
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedPe32 {
    pub kind: ImageKind32,
    pub preferred_base: u32,
    pub image_size: u32,
    pub mapped_pages: u32,
    pub entry_rva: Option<u32>,
    pub header_size: u32,
    pub stack_reserve: u32,
    pub stack_commit: u32,
    pub heap_reserve: u32,
    pub heap_commit: u32,
    pub dll_characteristics: u16,
    pub sections: Vec<Section32>,
    pub imports: Vec<ImportModule32>,
    pub exports: Exports32,
    pub tls: Option<StaticTls32>,
    pub load_config: Option<LoadConfig32>,
    pub delay_import_directory: Option<Directory32>,
    pub(super) directories: [Option<Directory32>; 16],
    pub(super) relocation_directory: Option<Directory32>,
    pub(super) relocations: Vec<Relocation32>,
    pub(super) relocs_stripped: bool,
    pub(super) source_length: usize,
}

impl PreparedPe32 {
    pub(crate) fn resource_directory(&self) -> Option<Directory32> {
        self.directories[2]
    }
}
