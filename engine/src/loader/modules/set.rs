use std::sync::Arc;

use super::{ModuleError32, ModuleId32, catalog::ModuleSource32, normalize_module_name};
use crate::{
    cpu::dbt::GateSpec,
    loader::pe32::module::{
        Directory32, ImageKind32, ImportSymbol32, LoadConfig32, ParserLimits32,
    },
    memory::{AddressSpace, GuestAddress, PageRange},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleTls32 {
    pub template: Vec<u8>,
    pub zero_fill: u32,
    pub alignment: u32,
    pub index_address: GuestAddress,
    pub callbacks: Vec<GuestAddress>,
    pub callbacks_address: Option<GuestAddress>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolvedExport32 {
    GuestAddress {
        module: ModuleId32,
        address: GuestAddress,
    },
    SystemAddress {
        module: ModuleId32,
        address: GuestAddress,
    },
    SystemGate {
        module: ModuleId32,
        gate: GateSpec,
    },
}

impl ResolvedExport32 {
    pub fn address(self) -> GuestAddress {
        match self {
            Self::GuestAddress { address, .. } | Self::SystemAddress { address, .. } => address,
            Self::SystemGate { gate, .. } => gate.entry,
        }
    }

    pub fn module(self) -> ModuleId32 {
        match self {
            Self::GuestAddress { module, .. }
            | Self::SystemAddress { module, .. }
            | Self::SystemGate { module, .. } => module,
        }
    }
}

#[derive(Debug)]
pub struct LoadedModule32 {
    pub(super) id: ModuleId32,
    pub(super) source: Arc<ModuleSource32>,
    pub(super) base: u32,
    pub(super) image_size: u32,
    pub(super) ranges: Vec<PageRange>,
    pub(super) dependencies: Vec<ModuleId32>,
    pub(super) tls: Option<ModuleTls32>,
    pub(super) gates: Vec<GateSpec>,
}

impl LoadedModule32 {
    pub(super) fn parser_limits(&self) -> ParserLimits32 {
        match &*self.source {
            ModuleSource32::Guest { parser_limits, .. } => *parser_limits,
            ModuleSource32::System(_) => ParserLimits32::default(),
        }
    }

    pub fn id(&self) -> ModuleId32 {
        self.id
    }

    pub fn identity(&self) -> &str {
        self.source.name()
    }

    pub fn base(&self) -> u32 {
        self.base
    }

    pub fn image_size(&self) -> u32 {
        self.image_size
    }

    pub fn is_dll(&self) -> bool {
        match &*self.source {
            ModuleSource32::Guest { image, .. } => image.kind == ImageKind32::Dll,
            ModuleSource32::System(_) => true,
        }
    }

    pub fn is_system(&self) -> bool {
        matches!(&*self.source, ModuleSource32::System(_))
    }

    pub fn entry(&self) -> Option<GuestAddress> {
        match &*self.source {
            ModuleSource32::Guest { image, .. } => {
                image.entry_rva.map(|rva| GuestAddress(self.base + rva))
            }
            ModuleSource32::System(_) => None,
        }
    }

    pub fn mapped_ranges(&self) -> &[PageRange] {
        &self.ranges
    }

    pub fn dependencies(&self) -> &[ModuleId32] {
        &self.dependencies
    }

    pub fn tls(&self) -> Option<&ModuleTls32> {
        self.tls.as_ref()
    }

    pub fn gates(&self) -> &[GateSpec] {
        &self.gates
    }

    pub fn dll_characteristics(&self) -> u16 {
        match &*self.source {
            ModuleSource32::Guest { image, .. } => image.dll_characteristics,
            ModuleSource32::System(_) => 0,
        }
    }

    pub fn stack_sizes(&self) -> Option<(u32, u32)> {
        match &*self.source {
            ModuleSource32::Guest { image, .. } => Some((image.stack_reserve, image.stack_commit)),
            ModuleSource32::System(_) => None,
        }
    }

    pub fn resource_directory(&self) -> Option<crate::loader::Directory32> {
        match &*self.source {
            ModuleSource32::Guest { image, .. } => image.resource_directory(),
            ModuleSource32::System(_) => None,
        }
    }

    pub fn load_config(&self) -> Option<&LoadConfig32> {
        match &*self.source {
            ModuleSource32::Guest { image, .. } => image.load_config.as_ref(),
            ModuleSource32::System(_) => None,
        }
    }

    pub fn delay_import_directory(&self) -> Option<Directory32> {
        match &*self.source {
            ModuleSource32::Guest { image, .. } => image.delay_import_directory,
            ModuleSource32::System(_) => None,
        }
    }
}

#[derive(Debug)]
pub struct ModuleSet32 {
    pub(super) identity: Arc<()>,
    pub(super) modules: Vec<LoadedModule32>,
    pub(super) revision: u64,
    pub(super) next_id: u64,
    pub(super) datafile_count: usize,
    pub(super) root: Option<ModuleId32>,
    pub(super) forwarder_limit: usize,
}

impl Default for ModuleSet32 {
    fn default() -> Self {
        Self {
            identity: Arc::new(()),
            modules: Vec::new(),
            revision: 0,
            next_id: 0,
            datafile_count: 0,
            root: None,
            forwarder_limit: super::ModuleLimits32::default().forwarder_depth,
        }
    }
}

impl ModuleSet32 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &LoadedModule32> {
        self.modules.iter()
    }

    pub fn get(&self, id: ModuleId32) -> Option<&LoadedModule32> {
        self.modules.iter().find(|module| module.id == id)
    }

    pub fn find(&self, name: &str) -> Option<&LoadedModule32> {
        let name = normalize_module_name(name).ok()?;
        self.find_normalized(&name)
    }

    pub fn containing(&self, address: GuestAddress) -> Option<&LoadedModule32> {
        self.modules.iter().find(|module| {
            u64::from(address.0) >= u64::from(module.base)
                && u64::from(address.0) < u64::from(module.base) + u64::from(module.image_size)
        })
    }

    pub fn root(&self) -> Option<ModuleId32> {
        self.root
    }

    pub fn intersects(&self, address: GuestAddress, length: u32) -> bool {
        let start = u64::from(address.0);
        let end = start + u64::from(length);
        self.modules.iter().any(|module| {
            start < u64::from(module.base) + u64::from(module.image_size)
                && u64::from(module.base) < end
        })
    }

    pub fn gate(&self, address: GuestAddress) -> Option<GateSpec> {
        self.modules
            .iter()
            .flat_map(|module| &module.gates)
            .find(|gate| gate.entry == address)
            .copied()
    }

    pub fn resolve_export(
        &self,
        id: ModuleId32,
        symbol: &ImportSymbol32,
    ) -> Result<ResolvedExport32, ModuleError32> {
        super::linking::resolve_loaded(self, id, symbol)
    }

    pub fn resolve_export_live(
        &self,
        memory: &AddressSpace,
        id: ModuleId32,
        symbol: &ImportSymbol32,
    ) -> Result<ResolvedExport32, ModuleError32> {
        super::linking::resolve_loaded_live(self, memory, id, symbol)
    }

    pub(crate) fn direct_export_live(
        &self,
        memory: &AddressSpace,
        id: ModuleId32,
        symbol: &ImportSymbol32,
    ) -> Result<Option<ResolvedExport32>, ModuleError32> {
        super::linking::direct_loaded_live(self, memory, id, symbol)
    }

    pub(super) fn find_normalized(&self, name: &str) -> Option<&LoadedModule32> {
        self.modules.iter().find(|module| module.identity() == name)
    }
}
