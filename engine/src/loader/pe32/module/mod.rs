mod resources;
pub use resources::{
    PeResourceFile32, PeResources32, ResourceError32, ResourceFileRange32, ResourceFileSpan32,
    ResourceIdentifier32, ResourceImageRead32, ResourceLocation32, ResourceString32,
};
mod config;
mod exports;
mod headers;
mod imports;
mod mapping;
mod metadata;
mod read;
mod relocation;
mod tls;

pub(crate) use mapping::{copy_into, copy_out};
pub use metadata::{
    Directory32, ImageKind32, ImportSymbol32, LoadConfig32, ParserLimits32, PreparedImagePage32,
    StaticTls32,
};
pub(crate) use metadata::{
    ExportTarget32, Exports32, ImportModule32, ImportSlot32, PreparedPe32, Section32,
};

use crate::loader::LoadError;

#[cfg(test)]
pub(crate) fn parse_module(bytes: &[u8]) -> Result<PreparedPe32, LoadError> {
    parse_module_with_limits(bytes, ParserLimits32::default())
}

pub(crate) fn parse_module_with_limits(
    bytes: &[u8],
    limits: ParserLimits32,
) -> Result<PreparedPe32, LoadError> {
    #[cfg(feature = "observability")]
    let _scope = ring3_observability::current().scope(
        ring3_observability::Level::Detailed,
        ring3_observability::categories::ENGINE_LOADER_PE32,
        "parse_module",
        &[ring3_observability::Field::unsigned(
            "bytes",
            bytes.len() as u64,
        )],
    );
    let mut budget = read::ParseBudget::new(limits);
    let mut image = headers::parse(bytes, &mut budget)?;
    image.imports = imports::parse(bytes, &image, &mut budget)?;
    image.exports = exports::parse(bytes, &image, &mut budget)?;
    image.tls = tls::parse(bytes, &image, &mut budget)?;
    image.load_config = config::parse(bytes, &image, &mut budget)?;
    image.relocations = relocation::parse(bytes, &image, &mut budget)?;
    Ok(image)
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
