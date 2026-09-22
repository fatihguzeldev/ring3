use std::ops::Range;

use super::{
    GuestMemory, Image, ImportPolicy, LoadError, LoadedPe32, imports, placement, relocations,
};
use crate::{
    PeExportLookup, PeExportQuery, PeExportSelection, PeExportTarget, PeImportSymbol,
    parse_pe_import_lookups,
};

/// an explicit dll provider, matched case-insensitively without host file lookup.
/// names contain 1..=255 ascii letters, digits, dots, underscores or hyphens;
/// single/double dots are invalid. loading accepts at most 16 providers/128 mib.
#[derive(Clone, Copy, Debug)]
pub struct GuestModule<'a> {
    pub name: &'a str,
    pub bytes: &'a [u8],
}

pub(in crate::execution) struct Initializer {
    pub name: String,
    pub base: u32,
    pub entry: u32,
}

pub(in crate::execution) struct MappedModule {
    pub name: String,
    pub base: u32,
}

pub(in crate::execution) struct LoadedModules {
    pub image: LoadedPe32,
    pub initializers: Vec<Initializer>,
    pub providers: Vec<MappedModule>,
    pub subsystem_version: (u16, u16),
}

pub(in crate::execution) fn load_modules(
    bytes: &[u8],
    page_limit: u32,
    modules: &[GuestModule<'_>],
    startup_count: usize,
    reserved: &[Range<u64>],
    mut fallback: impl FnMut(&str, PeImportSymbol<'_>) -> Option<u32>,
) -> Result<LoadedModules, LoadError> {
    validate_modules(modules)?;
    validate_deferred_dependencies(bytes, modules, startup_count)?;
    let program = Image::parse(bytes, ImportPolicy::GuestManagedDelay, false)?;
    let mut images = modules
        .iter()
        .map(|module| Image::parse(module.bytes, ImportPolicy::GuestManagedDelay, true))
        .collect::<Result<Vec<_>, _>>()?;
    // null cannot identify a resident module or the main executable.
    if std::iter::once(&program)
        .chain(&images)
        .any(|image| image.base() == 0)
    {
        return Err(LoadError::InvalidLayout);
    }
    placement::assign(&program, &mut images, reserved)?;
    let order = initialization_order(modules)?;
    let mut exports: Vec<_> = modules
        .iter()
        .map(|module| PeExportLookup::new(module.bytes))
        .collect();
    let mut memory = GuestMemory::new(page_limit);
    for image in std::iter::once(&program).chain(&images) {
        image.map(&mut memory)?;
    }
    for image in &images {
        relocations::apply(image, &mut memory)?;
    }
    for image in std::iter::once(&program).chain(&images) {
        imports::bind(image.bytes, image.base(), &mut memory, |name, symbol| {
            let Some(index) = find(modules, name) else {
                return Ok(fallback(name, symbol));
            };
            let query = match symbol {
                PeImportSymbol::ByName { name, .. } => PeExportQuery::Name(name),
                PeImportSymbol::Ordinal(ordinal) => PeExportQuery::Ordinal(u32::from(ordinal)),
            };
            let selection = exports[index]
                .lookup(query)
                .map_err(|cause| LoadError::Exports {
                    module: name.to_owned(),
                    cause,
                })?;
            if let PeExportSelection::Selected { address, .. } = selection
                && let PeExportTarget::Rva(rva) = address.target
                && rva.get() < images[index].table.headers.optional.size_of_image
            {
                return Ok(Some(
                    u32::try_from(images[index].base() + u64::from(rva.get()))
                        .expect("validated pe32 export address"),
                ));
            }
            Err(LoadError::InvalidExport {
                module: name.to_owned(),
                symbol: match symbol {
                    PeImportSymbol::ByName { name, .. } => name.to_owned(),
                    PeImportSymbol::Ordinal(value) => format!("#{value}"),
                },
            })
        })?;
    }
    for image in std::iter::once(&program).chain(&images) {
        image.protect(&mut memory)?;
    }
    let initializers = order
        .into_iter()
        .filter(|&index| images[index].has_entry())
        .map(|index| Initializer {
            name: modules[index].name.to_owned(),
            base: u32::try_from(images[index].base()).expect("validated pe32 base"),
            entry: images[index].entry_point(),
        })
        .collect();
    let providers = modules
        .iter()
        .zip(&images)
        .map(|(module, image)| MappedModule {
            name: module.name.to_owned(),
            base: u32::try_from(image.base()).expect("validated pe32 base"),
        })
        .collect();
    Ok(LoadedModules {
        image: LoadedPe32 {
            memory,
            image_base: u32::try_from(program.base()).expect("validated pe32 base"),
            entry_point: program.entry_point(),
        },
        initializers,
        providers,
        subsystem_version: program.table.headers.optional.subsystem_version,
    })
}

fn validate_deferred_dependencies(
    program: &[u8],
    modules: &[GuestModule<'_>],
    startup_count: usize,
) -> Result<(), LoadError> {
    let deferred = &modules[startup_count..];
    if deferred.is_empty() {
        return Ok(());
    }
    for (name, bytes) in std::iter::once(("<program>", program))
        .chain(modules.iter().map(|module| (module.name, module.bytes)))
    {
        for import in parse_pe_import_lookups(bytes).map_err(LoadError::Imports)? {
            if let Some(index) = find(deferred, import.descriptor.dll_name) {
                return Err(LoadError::DeferredModuleDependency {
                    module: name.to_owned(),
                    dependency: deferred[index].name.to_owned(),
                });
            }
        }
    }
    Ok(())
}

fn validate_modules(modules: &[GuestModule<'_>]) -> Result<(), LoadError> {
    if modules.len() > 16 {
        return Err(LoadError::ModuleLimitExceeded);
    }
    let mut total = 0_usize;
    for (index, module) in modules.iter().enumerate() {
        total = total
            .checked_add(module.bytes.len())
            .filter(|&size| size <= 128 * 1024 * 1024)
            .ok_or(LoadError::ModuleLimitExceeded)?;
        if module.name.is_empty()
            || module.name.len() > 255
            || matches!(module.name, "." | "..")
            || !module
                .name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(LoadError::InvalidModuleName);
        }
        if find(&modules[..index], module.name).is_some() {
            return Err(LoadError::DuplicateModule {
                name: module.name.to_owned(),
            });
        }
    }
    Ok(())
}

fn find(modules: &[GuestModule<'_>], name: &str) -> Option<usize> {
    modules
        .iter()
        .position(|module| module.name.eq_ignore_ascii_case(name))
}

fn initialization_order(modules: &[GuestModule<'_>]) -> Result<Vec<usize>, LoadError> {
    let mut edges = vec![Vec::new(); modules.len()];
    for (index, module) in modules.iter().enumerate() {
        for import in parse_pe_import_lookups(module.bytes).map_err(LoadError::Imports)? {
            if let Some(dependency) = find(modules, import.descriptor.dll_name)
                && !edges[index].contains(&dependency)
            {
                edges[index].push(dependency);
            }
        }
    }
    let mut order = Vec::new();
    let mut visiting = [false; 16];
    for index in 0..modules.len() {
        visit(index, &edges, modules, &mut visiting, &mut order)?;
    }
    Ok(order)
}

fn visit(
    index: usize,
    edges: &[Vec<usize>],
    modules: &[GuestModule<'_>],
    visiting: &mut [bool; 16],
    order: &mut Vec<usize>,
) -> Result<(), LoadError> {
    if order.contains(&index) {
        return Ok(());
    }
    if visiting[index] {
        return Err(LoadError::CyclicModules {
            module: modules[index].name.to_owned(),
        });
    }
    visiting[index] = true;
    for &dependency in &edges[index] {
        visit(dependency, edges, modules, visiting, order)?;
    }
    visiting[index] = false;
    order.push(index);
    Ok(())
}
