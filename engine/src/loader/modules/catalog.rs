use std::sync::Arc;

use super::{ModuleError32, normalize_module_identity, normalize_module_name};
use crate::{
    loader::pe32::module::{ParserLimits32, PreparedPe32, parse_module_with_limits},
    memory::GuestAddress,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SystemExportTarget32 {
    Gate(u32),
    PrivateGate(u32),
    Address(GuestAddress),
    Forwarder(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemExport32 {
    pub name: Option<String>,
    pub ordinal: Option<u16>,
    pub target: SystemExportTarget32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemModule32 {
    pub name: String,
    pub preferred_base: u32,
    pub exports: Vec<SystemExport32>,
}

#[derive(Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "sources already share one allocation through Arc"
)]
pub(super) enum ModuleSource32 {
    Guest {
        name: String,
        path: Option<String>,
        bytes: Box<[u8]>,
        image: PreparedPe32,
        parser_limits: ParserLimits32,
    },
    System(SystemModule32),
}

impl ModuleSource32 {
    pub(super) fn name(&self) -> &str {
        match self {
            Self::Guest { name, .. } => name,
            Self::System(module) => &module.name,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModuleLimits32 {
    pub modules: usize,
    pub source_bytes: usize,
    pub forwarder_depth: usize,
    pub parser: ParserLimits32,
}

impl Default for ModuleLimits32 {
    fn default() -> Self {
        Self {
            modules: 256,
            source_bytes: 256 * 1024 * 1024,
            forwarder_depth: 4096,
            parser: ParserLimits32::default(),
        }
    }
}

#[derive(Default, Debug)]
pub struct ModuleCatalog32 {
    pub(super) sources: Vec<Arc<ModuleSource32>>,
    pub(super) limits: ModuleLimits32,
    source_bytes: usize,
}

impl ModuleCatalog32 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    pub fn with_limits(limits: ModuleLimits32) -> Result<Self, ModuleError32> {
        if limits.modules == 0
            || limits.modules > u32::MAX as usize
            || limits.source_bytes == 0
            || limits.source_bytes > u32::MAX as usize
            || limits.forwarder_depth == 0
            || limits.forwarder_depth > u32::MAX as usize
            || limits.parser.metadata_entries == 0
            || limits.parser.metadata_entries > u32::MAX as usize
            || limits.parser.string_bytes == 0
            || limits.parser.string_bytes > u32::MAX as usize
        {
            return Err(ModuleError32::InvalidRequest);
        }
        Ok(Self {
            sources: Vec::new(),
            limits,
            source_bytes: 0,
        })
    }

    pub fn limits(&self) -> ModuleLimits32 {
        self.limits
    }

    pub fn remaining_source_bytes(&self) -> usize {
        self.limits.source_bytes.saturating_sub(self.source_bytes)
    }

    pub fn add_guest(&mut self, name: &str, bytes: &[u8]) -> Result<(), ModuleError32> {
        let path = super::identity::module_source_path(name)?;
        let name = self.available_name(name)?;
        let source_bytes = self
            .source_bytes
            .checked_add(bytes.len())
            .ok_or(ModuleError32::Capacity)?;
        if source_bytes > self.limits.source_bytes {
            return Err(ModuleError32::Capacity);
        }
        let image = parse_module_with_limits(bytes, self.limits.parser)?;
        self.sources
            .try_reserve(1)
            .map_err(|_| ModuleError32::Capacity)?;
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(bytes.len())
            .map_err(|_| ModuleError32::Capacity)?;
        owned.extend_from_slice(bytes);
        self.sources.push(Arc::new(ModuleSource32::Guest {
            name,
            path,
            bytes: owned.into_boxed_slice(),
            image,
            parser_limits: self.limits.parser,
        }));
        self.source_bytes = source_bytes;
        Ok(())
    }

    pub fn replace_guest(&mut self, name: &str, bytes: &[u8]) -> Result<(), ModuleError32> {
        let name = normalize_module_identity(name)?;
        let slot = self
            .sources
            .iter()
            .position(|source| source.name() == name)
            .ok_or(ModuleError32::MissingModule)?;
        let source = &self.sources[slot];
        let ModuleSource32::Guest {
            bytes: old, path, ..
        } = &**source
        else {
            return Err(ModuleError32::InvalidModuleKind);
        };
        if Arc::strong_count(source) != 1 {
            return Err(ModuleError32::ModuleInUse);
        }
        let source_bytes = (self.source_bytes - old.len())
            .checked_add(bytes.len())
            .ok_or(ModuleError32::Capacity)?;
        if source_bytes > self.limits.source_bytes {
            return Err(ModuleError32::Capacity);
        }
        let image = parse_module_with_limits(bytes, self.limits.parser)?;
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(bytes.len())
            .map_err(|_| ModuleError32::Allocation)?;
        owned.extend_from_slice(bytes);
        let replacement = Arc::new(ModuleSource32::Guest {
            name,
            path: path.clone(),
            bytes: owned.into_boxed_slice(),
            image,
            parser_limits: self.limits.parser,
        });
        self.sources[slot] = replacement;
        self.source_bytes = source_bytes;
        Ok(())
    }

    pub fn remove_guest(&mut self, name: &str) -> Result<(), ModuleError32> {
        let name = normalize_module_identity(name)?;
        let slot = self
            .sources
            .iter()
            .position(|source| source.name() == name)
            .ok_or(ModuleError32::MissingModule)?;
        let source = &self.sources[slot];
        let ModuleSource32::Guest { bytes, .. } = &**source else {
            return Err(ModuleError32::InvalidModuleKind);
        };
        if Arc::strong_count(source) != 1 {
            return Err(ModuleError32::ModuleInUse);
        }
        self.source_bytes -= bytes.len();
        self.sources.remove(slot);
        Ok(())
    }

    /// Register a provider and its facade dependencies without partial admission.
    pub fn add_system_group(&mut self, modules: Vec<SystemModule32>) -> Result<(), ModuleError32> {
        let capacity = self
            .sources
            .len()
            .checked_add(modules.len())
            .ok_or(ModuleError32::Capacity)?;
        let mut sources = Vec::new();
        sources
            .try_reserve_exact(capacity)
            .map_err(|_| ModuleError32::Allocation)?;
        sources.extend(self.sources.iter().cloned());
        let mut candidate = Self {
            sources,
            limits: self.limits,
            source_bytes: self.source_bytes,
        };
        for module in modules {
            candidate.add_system(module)?;
        }
        *self = candidate;
        Ok(())
    }
    pub fn add_system(&mut self, mut module: SystemModule32) -> Result<(), ModuleError32> {
        normalize_module_name(&module.name)?;
        module.name = self.available_name(&module.name)?;
        if module.preferred_base == 0 || !module.preferred_base.is_multiple_of(65536) {
            return Err(ModuleError32::InvalidSystemExport);
        }
        for (index, export) in module.exports.iter().enumerate() {
            if matches!(export.target, SystemExportTarget32::PrivateGate(_))
                != (export.name.is_none() && export.ordinal.is_none())
            {
                return Err(ModuleError32::InvalidSystemExport);
            }
            if let Some(name) = &export.name
                && (name.is_empty() || name.bytes().any(|byte| byte == 0))
            {
                return Err(ModuleError32::InvalidSystemExport);
            }
            if module.exports[..index].iter().any(|previous| {
                export.name.is_some() && export.name == previous.name
                    || export.ordinal.is_some() && export.ordinal == previous.ordinal
            }) {
                return Err(ModuleError32::DuplicateSymbol);
            }
            match &export.target {
                SystemExportTarget32::Gate(0)
                | SystemExportTarget32::PrivateGate(0)
                | SystemExportTarget32::Address(GuestAddress(0)) => {
                    return Err(ModuleError32::InvalidSystemExport);
                }
                SystemExportTarget32::Forwarder(forwarder) => {
                    super::linking::parse_forwarder(forwarder)?;
                }
                _ => {}
            }
        }
        self.sources
            .try_reserve(1)
            .map_err(|_| ModuleError32::Capacity)?;
        self.sources.push(Arc::new(ModuleSource32::System(module)));
        Ok(())
    }

    pub fn contains(&self, name: &str) -> bool {
        normalize_module_identity(name)
            .is_ok_and(|name| self.sources.iter().any(|source| source.name() == name))
    }

    pub fn is_system(&self, name: &str) -> bool {
        normalize_module_identity(name).is_ok_and(|name| {
            self.sources.iter().any(|source| {
                source.name() == name && matches!(&**source, ModuleSource32::System(_))
            })
        })
    }

    pub(super) fn source(&self, name: &str) -> Result<Arc<ModuleSource32>, ModuleError32> {
        self.sources
            .iter()
            .find(|source| source.name() == name)
            .cloned()
            .ok_or(ModuleError32::MissingModule)
    }

    fn available_name(&self, name: &str) -> Result<String, ModuleError32> {
        let name = normalize_module_identity(name)?;
        if self.sources.iter().any(|source| source.name() == name) {
            return Err(ModuleError32::DuplicateModule);
        }
        if self.sources.len() == self.limits.modules {
            return Err(ModuleError32::Capacity);
        }
        Ok(name)
    }
}

#[cfg(test)]
mod tests;
