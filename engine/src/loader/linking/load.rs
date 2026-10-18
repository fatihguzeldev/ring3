use super::image::{LinkedMetadata32, LoadedLinkedImage32};
use crate::{
    loader::{
        LoadError, ProviderProfile,
        imports::prepare_imports,
        pe32::{MAX_IMAGE_BYTES, initialize, parse_linked, protect},
        relocation::prepare_fixups,
    },
    memory::{AddressSpace, GuestAddress, MAX_RESIDENT_PAGES, PAGE_SIZE, Permissions},
};

pub(crate) fn load_linked_profile(
    bytes: &[u8],
    actual_base: u32,
    gate_base: u32,
    resident_pages: u32,
    profile: ProviderProfile,
) -> Result<LoadedLinkedImage32, LoadError> {
    #[cfg(feature = "observability")]
    let _scope = ring3_observability::current().scope(
        ring3_observability::Level::Basic,
        "engine.loader.linker",
        "load",
        &[ring3_observability::Field::unsigned(
            "bytes",
            bytes.len() as u64,
        )],
    );
    if !(1..=MAX_RESIDENT_PAGES).contains(&resident_pages) || bytes.len() > MAX_IMAGE_BYTES as usize
    {
        return Err(LoadError::Capacity);
    }
    if actual_base == 0 || !actual_base.is_multiple_of(65536) {
        return Err(LoadError::Malformed);
    }
    let mut plan = parse_linked(bytes)?;
    let image_end = u64::from(actual_base) + u64::from(plan.metadata.image_size);
    let gate_end = u64::from(gate_base) + u64::from(PAGE_SIZE);
    if image_end > 1 << 32
        || gate_base == 0
        || !gate_base.is_multiple_of(PAGE_SIZE)
        || gate_end > 1 << 32
        || (u64::from(gate_base) < image_end && u64::from(actual_base) < gate_end)
    {
        return Err(LoadError::Malformed);
    }
    let mapped_pages = plan
        .metadata
        .mapped_pages
        .checked_add(1)
        .ok_or(LoadError::Capacity)?;
    if mapped_pages > resident_pages {
        return Err(LoadError::Capacity);
    }
    let preferred_base = plan.metadata.image_base;
    let delta = actual_base.wrapping_sub(preferred_base);
    if delta != 0 && plan.relocation_directory.is_none() {
        return Err(LoadError::Unsupported);
    }
    let imports = prepare_imports(bytes, &plan, plan.import_directory, plan.iat_directory)?;
    let fixups = prepare_fixups(bytes, &plan, plan.relocation_directory, delta)?;
    imports.validate_aliases(plan.relocation_directory, &fixups)?;
    let imports = imports.resolve(gate_base, profile)?;

    let mut memory = AddressSpace::new(resident_pages).map_err(LoadError::Memory)?;
    initialize(
        &mut memory,
        actual_base,
        plan.header_pages,
        &bytes[..plan.header_size],
    )?;
    for section in &plan.sections[..plan.section_count] {
        initialize(
            &mut memory,
            actual_base + section.rva,
            section.pages,
            &bytes[section.raw_pointer..section.raw_pointer + section.raw_size],
        )?;
    }
    initialize(&mut memory, gate_base, 1, &[])?;
    for fixup in fixups {
        memory
            .write(
                GuestAddress(actual_base + fixup.rva),
                &fixup.value.to_le_bytes(),
            )
            .map_err(LoadError::Memory)?;
    }
    for (index, address) in imports.slots[..imports.slot_count].iter().enumerate() {
        memory
            .write(
                GuestAddress(actual_base + imports.iat_rva + index as u32 * 4),
                &address.to_le_bytes(),
            )
            .map_err(LoadError::Memory)?;
    }
    for gate in &imports.gates[..imports.gate_count as usize] {
        memory
            .write(gate.entry, &[0x0f, 0x0b])
            .map_err(LoadError::Memory)?;
    }
    protect(
        &mut memory,
        actual_base,
        plan.header_pages,
        Permissions::READ,
    )?;
    for section in &plan.sections[..plan.section_count] {
        protect(
            &mut memory,
            actual_base + section.rva,
            section.pages,
            section.permissions,
        )?;
    }
    protect(&mut memory, gate_base, 1, Permissions::READ_EXECUTE)?;
    plan.metadata.entry_point = actual_base + (plan.metadata.entry_point - preferred_base);
    plan.metadata.image_base = actual_base;
    plan.metadata.mapped_pages = mapped_pages;
    Ok(LoadedLinkedImage32 {
        memory,
        metadata: LinkedMetadata32 {
            image: plan.metadata,
            gate_base,
            gate_count: imports.gate_count,
            gates: imports.gates,
        },
    })
}
