mod live;

use super::{
    LoadedModule32, ModuleError32, ModuleId32, ModuleSet32, ResolvedExport32, SystemExportTarget32,
    catalog::ModuleSource32, normalize_module_name,
};
use crate::{
    loader::pe32::module::{ExportTarget32, ImportSymbol32},
    memory::{AddressSpace, GuestAddress},
};

pub(super) enum ExportValue32 {
    Resolved(ResolvedExport32),
    Forwarder(String),
}

pub(super) fn parse_forwarder(value: &str) -> Result<(String, ImportSymbol32), ModuleError32> {
    let (module, symbol) = value
        .rsplit_once('.')
        .ok_or(ModuleError32::InvalidForwarder)?;
    let module = normalize_module_name(module).map_err(|_| ModuleError32::InvalidForwarder)?;
    let symbol = if let Some(ordinal) = symbol.strip_prefix('#') {
        if ordinal.is_empty() || !ordinal.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(ModuleError32::InvalidForwarder);
        }
        ImportSymbol32::Ordinal(
            ordinal
                .parse()
                .map_err(|_| ModuleError32::InvalidForwarder)?,
        )
    } else {
        if symbol.is_empty() || symbol.bytes().any(|byte| !(0x20..=0x7e).contains(&byte)) {
            return Err(ModuleError32::InvalidForwarder);
        }
        ImportSymbol32::Name(symbol.to_owned())
    };
    Ok((module, symbol))
}

pub(super) fn export_value(
    module: &LoadedModule32,
    symbol: &ImportSymbol32,
) -> Result<ExportValue32, ModuleError32> {
    match &*module.source {
        ModuleSource32::Guest { image, .. } => match image.exports.lookup(symbol) {
            Some(ExportTarget32::Address(rva)) => {
                Ok(ExportValue32::Resolved(ResolvedExport32::GuestAddress {
                    module: module.id,
                    address: GuestAddress(module.base + rva),
                }))
            }
            Some(ExportTarget32::Forwarder(value)) => Ok(ExportValue32::Forwarder(value.clone())),
            None => Err(ModuleError32::MissingSymbol),
        },
        ModuleSource32::System(source) => {
            let export = source
                .exports
                .iter()
                .find(|export| match symbol {
                    ImportSymbol32::Name(name) => export.name.as_ref() == Some(name),
                    ImportSymbol32::Ordinal(ordinal) => export.ordinal == Some(*ordinal),
                })
                .ok_or(ModuleError32::MissingSymbol)?;
            match &export.target {
                SystemExportTarget32::Address(address) => {
                    Ok(ExportValue32::Resolved(ResolvedExport32::SystemAddress {
                        module: module.id,
                        address: *address,
                    }))
                }
                SystemExportTarget32::Gate(id) => {
                    let gate = module
                        .gates
                        .iter()
                        .find(|gate| gate.id == *id)
                        .ok_or(ModuleError32::InvalidSystemExport)?;
                    Ok(ExportValue32::Resolved(ResolvedExport32::SystemGate {
                        module: module.id,
                        gate: *gate,
                    }))
                }
                SystemExportTarget32::PrivateGate(_) => Err(ModuleError32::MissingSymbol),
                SystemExportTarget32::Forwarder(value) => {
                    Ok(ExportValue32::Forwarder(value.clone()))
                }
            }
        }
    }
}

pub(super) fn resolve_loaded(
    set: &ModuleSet32,
    id: ModuleId32,
    symbol: &ImportSymbol32,
) -> Result<ResolvedExport32, ModuleError32> {
    follow_loaded(set, id, symbol, export_value)
}

pub(super) fn resolve_loaded_live(
    set: &ModuleSet32,
    memory: &AddressSpace,
    id: ModuleId32,
    symbol: &ImportSymbol32,
) -> Result<ResolvedExport32, ModuleError32> {
    follow_loaded(set, id, symbol, |module, symbol| {
        export_value_live(memory, module, symbol)
    })
}

pub(super) fn direct_loaded_live(
    set: &ModuleSet32,
    memory: &AddressSpace,
    id: ModuleId32,
    symbol: &ImportSymbol32,
) -> Result<Option<ResolvedExport32>, ModuleError32> {
    let module = set.get(id).ok_or(ModuleError32::NotFound)?;
    match export_value_live(memory, module, symbol)? {
        ExportValue32::Resolved(value) => Ok(Some(value)),
        ExportValue32::Forwarder(_) => Ok(None),
    }
}

pub(super) fn export_value_live(
    memory: &AddressSpace,
    module: &LoadedModule32,
    symbol: &ImportSymbol32,
) -> Result<ExportValue32, ModuleError32> {
    if module.is_system() {
        return export_value(module, symbol);
    }
    live::lookup(memory, module, symbol)
}

fn follow_loaded(
    set: &ModuleSet32,
    mut id: ModuleId32,
    symbol: &ImportSymbol32,
    mut lookup: impl FnMut(&LoadedModule32, &ImportSymbol32) -> Result<ExportValue32, ModuleError32>,
) -> Result<ResolvedExport32, ModuleError32> {
    let mut symbol = symbol.clone();
    let mut visited = Vec::new();
    loop {
        if visited
            .iter()
            .any(|(previous_id, previous)| *previous_id == id && *previous == symbol)
        {
            return Err(ModuleError32::ForwarderCycle);
        }
        if visited.len() == set.forwarder_limit {
            return Err(ModuleError32::Capacity);
        }
        visited
            .try_reserve(1)
            .map_err(|_| ModuleError32::Allocation)?;
        visited.push((id, symbol.clone()));
        let module = set.get(id).ok_or(ModuleError32::NotFound)?;
        match lookup(module, &symbol)? {
            ExportValue32::Resolved(value) => return Ok(value),
            ExportValue32::Forwarder(value) => {
                let (name, next) = parse_forwarder(&value)?;
                id = set
                    .find_normalized(&name)
                    .ok_or(ModuleError32::MissingModule)?
                    .id;
                symbol = next;
            }
        }
    }
}
