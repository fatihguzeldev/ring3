use super::{
    LoadError,
    pe32::ImagePlan,
    relocation::{Fixup, RelocationDirectory},
};
use crate::{cpu::dbt::GateSpec, memory::GuestAddress, windows::WindowsApi32};

const MAX_SLOTS: usize = 8;
const MAX_METADATA_RANGES: usize = 4 + MAX_SLOTS;

#[derive(Clone, Copy)]
pub(super) enum ProviderProfile {
    LastErrorOnly,
    LastErrorAndExit,
    MainImageHandle,
}

#[derive(Clone, Copy)]
pub(super) struct ImportDirectory {
    pub rva: u32,
    pub size: u32,
}

#[derive(Clone, Copy)]
struct MetadataRange {
    rva: u32,
    size: u32,
    hint_name: bool,
}

impl MetadataRange {
    const EMPTY: Self = Self {
        rva: 0,
        size: 0,
        hint_name: false,
    };

    fn overlaps(self, other: Self) -> bool {
        u64::from(self.rva) < u64::from(other.rva) + u64::from(other.size)
            && u64::from(other.rva) < u64::from(self.rva) + u64::from(self.size)
    }
}

pub(super) struct ImportPlan<'a> {
    module: &'a str,
    symbols: [&'a str; MAX_SLOTS],
    slot_count: usize,
    iat_rva: u32,
    ranges: [MetadataRange; MAX_METADATA_RANGES],
    range_count: usize,
}

pub(super) struct ResolvedImports {
    pub iat_rva: u32,
    pub slot_count: usize,
    pub slots: [u32; MAX_SLOTS],
    pub gate_count: u32,
    pub gates: [GateSpec; 4],
}

impl ImportPlan<'_> {
    fn add_range(&mut self, range: MetadataRange) -> Result<(), LoadError> {
        *self
            .ranges
            .get_mut(self.range_count)
            .ok_or(LoadError::Capacity)? = range;
        self.range_count += 1;
        Ok(())
    }

    pub(super) fn validate_aliases(
        &self,
        directory: Option<RelocationDirectory>,
        fixups: &[Fixup],
    ) -> Result<(), LoadError> {
        for (index, range) in self.ranges[..self.range_count].iter().enumerate() {
            for earlier in &self.ranges[..index] {
                if range.overlaps(*earlier)
                    && !(range.hint_name
                        && earlier.hint_name
                        && range.rva == earlier.rva
                        && range.size == earlier.size)
                {
                    return Err(LoadError::Malformed);
                }
            }
            if let Some(directory) = directory
                && range.overlaps(MetadataRange {
                    rva: directory.rva,
                    size: directory.size,
                    hint_name: false,
                })
            {
                return Err(LoadError::Malformed);
            }
            for fixup in fixups {
                if range.overlaps(MetadataRange {
                    rva: fixup.rva,
                    size: 4,
                    hint_name: false,
                }) {
                    return Err(LoadError::Malformed);
                }
            }
        }
        Ok(())
    }

    pub(super) fn resolve(
        &self,
        gate_base: u32,
        profile: ProviderProfile,
    ) -> Result<ResolvedImports, LoadError> {
        let mut resolved = ResolvedImports {
            iat_rva: self.iat_rva,
            slot_count: self.slot_count,
            slots: [0; MAX_SLOTS],
            gate_count: 0,
            gates: [GateSpec {
                entry: GuestAddress(0),
                id: 0,
            }; 4],
        };
        let mut used = [false; 4];
        for (index, symbol) in self.symbols[..self.slot_count].iter().enumerate() {
            let api = WindowsApi32::resolve(self.module, symbol).ok_or(LoadError::Unsupported)?;
            let gate_index = match api {
                WindowsApi32::GetLastError => 0,
                WindowsApi32::SetLastError => 1,
                WindowsApi32::ExitProcess => match profile {
                    ProviderProfile::LastErrorOnly => return Err(LoadError::Unsupported),
                    ProviderProfile::LastErrorAndExit | ProviderProfile::MainImageHandle => 2,
                },
                WindowsApi32::GetModuleHandleA => match profile {
                    ProviderProfile::MainImageHandle => 3,
                    ProviderProfile::LastErrorOnly | ProviderProfile::LastErrorAndExit => {
                        return Err(LoadError::Unsupported);
                    }
                },
            };
            used[gate_index] = true;
            resolved.slots[index] = gate_base + gate_index as u32 * 16;
        }
        for (index, api) in [
            WindowsApi32::GetLastError,
            WindowsApi32::SetLastError,
            WindowsApi32::ExitProcess,
            WindowsApi32::GetModuleHandleA,
        ]
        .into_iter()
        .enumerate()
        {
            if used[index] {
                resolved.gates[resolved.gate_count as usize] = GateSpec {
                    entry: GuestAddress(gate_base + index as u32 * 16),
                    id: api.id(),
                };
                resolved.gate_count += 1;
            }
        }
        Ok(resolved)
    }
}

pub(super) fn prepare_imports<'a>(
    bytes: &'a [u8],
    image: &ImagePlan,
    directory: Option<ImportDirectory>,
    iat_directory: Option<ImportDirectory>,
) -> Result<ImportPlan<'a>, LoadError> {
    let directory = directory.ok_or(LoadError::Unsupported)?;
    if !directory.rva.is_multiple_of(4) || directory.size < 40 || !directory.size.is_multiple_of(20)
    {
        return Err(LoadError::Malformed);
    }
    if directory.size > 40 {
        return Err(LoadError::Unsupported);
    }
    let descriptor = &bytes[image.file_range(directory.rva, directory.size)?];
    if descriptor[20..].iter().any(|byte| *byte != 0) {
        return Err(LoadError::Malformed);
    }
    let ilt_rva = word(descriptor, 0);
    let timestamp = word(descriptor, 4);
    let chain = word(descriptor, 8);
    let module_rva = word(descriptor, 12);
    let iat_rva = word(descriptor, 16);
    if ilt_rva == 0 || timestamp != 0 || !matches!(chain, 0 | u32::MAX) {
        return Err(LoadError::Unsupported);
    }
    if module_rva == 0
        || iat_rva == 0
        || !ilt_rva.is_multiple_of(4)
        || !iat_rva.is_multiple_of(4)
        || ilt_rva == iat_rva
    {
        return Err(LoadError::Malformed);
    }
    let (module, module_range) = ascii(bytes, image, module_rva, 0)?;
    let mut plan = ImportPlan {
        module,
        symbols: [""; MAX_SLOTS],
        slot_count: 0,
        iat_rva,
        ranges: [MetadataRange::EMPTY; MAX_METADATA_RANGES],
        range_count: 0,
    };
    plan.add_range(MetadataRange {
        rva: directory.rva,
        size: directory.size,
        hint_name: false,
    })?;
    plan.add_range(module_range)?;
    for index in 0..=MAX_SLOTS {
        let offset = index as u32 * 4;
        let size = offset + 4;
        let ilt_range = image.file_range(ilt_rva, size)?;
        let imported = word(bytes, ilt_range.end - 4);
        if imported != 0 && index == MAX_SLOTS {
            return Err(LoadError::Capacity);
        }
        if imported & 0x8000_0000 != 0 {
            return Err(LoadError::Unsupported);
        }
        let iat_range = image.file_range(iat_rva, size)?;
        if word(bytes, iat_range.end - 4) != imported {
            return Err(LoadError::Malformed);
        }
        if imported == 0 {
            if let Some(directory) = iat_directory
                && (directory.rva != iat_rva || directory.size != size)
            {
                return Err(LoadError::Malformed);
            }
            plan.add_range(MetadataRange {
                rva: ilt_rva,
                size,
                hint_name: false,
            })?;
            plan.add_range(MetadataRange {
                rva: iat_rva,
                size,
                hint_name: false,
            })?;
            if index == 0 {
                return Err(LoadError::Unsupported);
            }
            return Ok(plan);
        }
        if !imported.is_multiple_of(2) {
            return Err(LoadError::Malformed);
        }
        let (symbol, range) = ascii(bytes, image, imported, 2)?;
        plan.symbols[index] = symbol;
        plan.slot_count += 1;
        plan.add_range(range)?;
    }
    Err(LoadError::Capacity)
}

fn ascii<'a>(
    bytes: &'a [u8],
    image: &ImagePlan,
    rva: u32,
    prefix: u32,
) -> Result<(&'a str, MetadataRange), LoadError> {
    for length in 0..64 {
        let size = prefix + length + 1;
        let range = image.file_range(rva, size)?;
        let byte = bytes[range.end - 1];
        if byte == 0 {
            if length == 0 {
                return Err(LoadError::Malformed);
            }
            let name = std::str::from_utf8(&bytes[range.start + prefix as usize..range.end - 1])
                .map_err(|_| LoadError::Malformed)?;
            return Ok((
                name,
                MetadataRange {
                    rva,
                    size,
                    hint_name: prefix != 0,
                },
            ));
        }
        if !(0x20..=0x7e).contains(&byte) {
            return Err(LoadError::Malformed);
        }
        if length == 63 {
            // the cap does not require a byte beyond the initialized section.
            return Err(LoadError::Capacity);
        }
    }
    Err(LoadError::Capacity)
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}
