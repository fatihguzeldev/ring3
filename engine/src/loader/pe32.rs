use super::imports::{ImportDirectory, ProviderProfile, prepare_imports};
use super::relocation::{RelocationDirectory, prepare_fixups};
use super::{
    ImageMetadata32, LinkedImageMetadata32, LinkedImageMetadata32V2, LoadError, LoadedLinkedPe32,
    LoadedLinkedPe32V2, LoadedPe32,
};
use crate::memory::{AddressSpace, GuestAddress, PAGE_SIZE, PageRange, Permissions};

const MAX_IMAGE_BYTES: u32 = 16 * 1024 * 1024;
const FILE_ALIGNMENT: u32 = 512;
const SECTION_LIMIT: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Profile {
    Fixed,
    Relocated,
    Linked,
}

#[derive(Clone, Copy)]
struct Section {
    rva: u32,
    pages: u32,
    raw_pointer: usize,
    raw_size: usize,
    virtual_size: u32,
    permissions: Permissions,
}

impl Section {
    const EMPTY: Self = Self {
        rva: 0,
        pages: 0,
        raw_pointer: 0,
        raw_size: 0,
        virtual_size: 0,
        permissions: Permissions::NONE,
    };
}

pub(super) struct ImagePlan {
    metadata: ImageMetadata32,
    header_size: usize,
    header_pages: u32,
    sections: [Section; SECTION_LIMIT],
    section_count: usize,
    relocation_directory: Option<RelocationDirectory>,
    import_directory: Option<ImportDirectory>,
    iat_directory: Option<ImportDirectory>,
}

impl ImagePlan {
    pub(super) fn image_size(&self) -> u32 {
        self.metadata.image_size
    }

    pub(super) fn file_range(
        &self,
        rva: u32,
        length: u32,
    ) -> Result<std::ops::Range<usize>, LoadError> {
        for section in &self.sections[..self.section_count] {
            if let Some(offset) = rva.checked_sub(section.rva)
                && u64::from(offset) + u64::from(length)
                    <= u64::from(section.virtual_size.min(section.raw_size as u32))
            {
                let start = section.raw_pointer + offset as usize;
                return Ok(start..start + length as usize);
            }
        }
        Err(LoadError::Malformed)
    }
}

pub fn load_pe32(bytes: &[u8], resident_pages: u32) -> Result<LoadedPe32, LoadError> {
    if !(1..=4096).contains(&resident_pages) || bytes.len() > MAX_IMAGE_BYTES as usize {
        return Err(LoadError::Capacity);
    }
    let plan = parse(bytes, Profile::Fixed)?;
    if plan.metadata.mapped_pages > resident_pages {
        return Err(LoadError::Capacity);
    }
    let mut memory = AddressSpace::new(resident_pages).map_err(LoadError::Memory)?;
    stage(
        &mut memory,
        plan.metadata.image_base,
        plan.header_pages,
        &bytes[..plan.header_size],
        Permissions::READ,
    )?;
    for section in &plan.sections[..plan.section_count] {
        stage(
            &mut memory,
            plan.metadata.image_base + section.rva,
            section.pages,
            &bytes[section.raw_pointer..section.raw_pointer + section.raw_size],
            section.permissions,
        )?;
    }
    Ok(LoadedPe32 {
        memory,
        metadata: plan.metadata,
    })
}

pub fn load_pe32_at(
    bytes: &[u8],
    actual_base: u32,
    resident_pages: u32,
) -> Result<LoadedPe32, LoadError> {
    if !(1..=4096).contains(&resident_pages) || bytes.len() > MAX_IMAGE_BYTES as usize {
        return Err(LoadError::Capacity);
    }
    if actual_base == 0 || !actual_base.is_multiple_of(65536) {
        return Err(LoadError::Malformed);
    }
    let mut plan = parse(bytes, Profile::Relocated)?;
    if u64::from(actual_base) + u64::from(plan.metadata.image_size) > 1 << 32 {
        return Err(LoadError::Malformed);
    }
    if plan.metadata.mapped_pages > resident_pages {
        return Err(LoadError::Capacity);
    }
    let preferred_base = plan.metadata.image_base;
    let delta = actual_base.wrapping_sub(preferred_base);
    if delta != 0 && plan.relocation_directory.is_none() {
        return Err(LoadError::Unsupported);
    }
    let fixups = prepare_fixups(bytes, &plan, plan.relocation_directory, delta)?;
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
    for fixup in fixups {
        memory
            .write(
                GuestAddress(actual_base + fixup.rva),
                &fixup.value.to_le_bytes(),
            )
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
    plan.metadata.entry_point = actual_base + (plan.metadata.entry_point - preferred_base);
    plan.metadata.image_base = actual_base;
    Ok(LoadedPe32 {
        memory,
        metadata: plan.metadata,
    })
}

pub fn load_pe32_linked_at(
    bytes: &[u8],
    actual_base: u32,
    gate_base: u32,
    resident_pages: u32,
) -> Result<LoadedLinkedPe32, LoadError> {
    let linked = load_linked_profile(
        bytes,
        actual_base,
        gate_base,
        resident_pages,
        ProviderProfile::LastErrorOnly,
    )?;
    Ok(LoadedLinkedPe32 {
        memory: linked.memory,
        metadata: LinkedImageMetadata32 {
            image: linked.metadata.image,
            gate_base: linked.metadata.gate_base,
            gate_count: linked.metadata.gate_count,
            gates: [linked.metadata.gates[0], linked.metadata.gates[1]],
        },
    })
}

pub fn load_pe32_linked_v2_at(
    bytes: &[u8],
    actual_base: u32,
    gate_base: u32,
    resident_pages: u32,
) -> Result<LoadedLinkedPe32V2, LoadError> {
    load_linked_profile(
        bytes,
        actual_base,
        gate_base,
        resident_pages,
        ProviderProfile::LastErrorAndExit,
    )
}

fn load_linked_profile(
    bytes: &[u8],
    actual_base: u32,
    gate_base: u32,
    resident_pages: u32,
    profile: ProviderProfile,
) -> Result<LoadedLinkedPe32V2, LoadError> {
    if !(1..=4096).contains(&resident_pages) || bytes.len() > MAX_IMAGE_BYTES as usize {
        return Err(LoadError::Capacity);
    }
    if actual_base == 0 || !actual_base.is_multiple_of(65536) {
        return Err(LoadError::Malformed);
    }
    let mut plan = parse(bytes, Profile::Linked)?;
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
    Ok(LoadedLinkedPe32V2 {
        memory,
        metadata: LinkedImageMetadata32V2 {
            image: plan.metadata,
            gate_base,
            gate_count: imports.gate_count,
            gates: imports.gates,
        },
    })
}

fn stage(
    memory: &mut AddressSpace,
    address: u32,
    pages: u32,
    bytes: &[u8],
    permissions: Permissions,
) -> Result<(), LoadError> {
    initialize(memory, address, pages, bytes)?;
    protect(memory, address, pages, permissions)
}

fn initialize(
    memory: &mut AddressSpace,
    address: u32,
    pages: u32,
    bytes: &[u8],
) -> Result<(), LoadError> {
    let address = GuestAddress(address);
    let range = PageRange::new(address, pages).map_err(LoadError::Memory)?;
    memory
        .map_zeroed(range, Permissions::READ_WRITE)
        .map_err(LoadError::Memory)?;
    memory.write(address, bytes).map_err(LoadError::Memory)
}

fn protect(
    memory: &mut AddressSpace,
    address: u32,
    pages: u32,
    permissions: Permissions,
) -> Result<(), LoadError> {
    let range = PageRange::new(GuestAddress(address), pages).map_err(LoadError::Memory)?;
    memory
        .protect(range, permissions)
        .map_err(LoadError::Memory)
}

fn parse(bytes: &[u8], profile: Profile) -> Result<ImagePlan, LoadError> {
    if bytes.len() < 64 || bytes.get(..2) != Some(b"MZ") {
        return Err(LoadError::Malformed);
    }
    let pe = read32(bytes, 0x3c)? as usize;
    if pe < 64 || pe > bytes.len() {
        return Err(LoadError::Malformed);
    }
    let coff = pe + 4;
    let optional = coff + 20;
    if bytes.get(pe..coff) != Some(b"PE\0\0") || optional > bytes.len() {
        return Err(LoadError::Malformed);
    }
    if read16(bytes, coff)? != 0x14c || read16(bytes, coff + 16)? != 224 {
        return Err(LoadError::Unsupported);
    }
    let section_count = read16(bytes, coff + 2)? as usize;
    if section_count == 0 {
        return Err(LoadError::Malformed);
    }
    if section_count > SECTION_LIMIT {
        return Err(LoadError::Unsupported);
    }
    let characteristics = read16(bytes, coff + 18)?;
    if characteristics & 0x0102 != 0x0102
        || characteristics & !0x0123 != 0
        || read32(bytes, coff + 8)? != 0
        || read32(bytes, coff + 12)? != 0
    {
        return Err(LoadError::Unsupported);
    }
    let table = optional + 224;
    let table_end = table + section_count * 40;
    if table_end > bytes.len() {
        return Err(LoadError::Malformed);
    }
    if read16(bytes, optional)? != 0x10b
        || read32(bytes, optional + 32)? != PAGE_SIZE
        || read32(bytes, optional + 36)? != FILE_ALIGNMENT
        || read32(bytes, optional + 52)? != 0
        || !matches!(read16(bytes, optional + 68)?, 2 | 3)
        || read16(bytes, optional + 70)? & !0x0100 != 0
        || read32(bytes, optional + 88)? != 0
        || read32(bytes, optional + 92)? != 16
    {
        return Err(LoadError::Unsupported);
    }
    let mut relocation_directory = None;
    let mut import_directory = None;
    let mut iat_directory = None;
    for index in 0..16 {
        let rva = read32(bytes, optional + 96 + index * 8)?;
        let size = read32(bytes, optional + 100 + index * 8)?;
        if rva == 0 && size == 0 {
            continue;
        }
        if profile == Profile::Linked && matches!(index, 1 | 12) {
            if rva == 0 || size == 0 {
                return Err(LoadError::Malformed);
            }
            let directory = ImportDirectory { rva, size };
            if index == 1 {
                import_directory = Some(directory);
            } else {
                iat_directory = Some(directory);
            }
            continue;
        }
        if profile == Profile::Fixed || index != 5 {
            return Err(LoadError::Unsupported);
        }
        if rva == 0 || size == 0 || characteristics & 1 != 0 {
            return Err(LoadError::Malformed);
        }
        relocation_directory = Some(RelocationDirectory { rva, size });
    }
    let image_base = read32(bytes, optional + 28)?;
    let image_size = read32(bytes, optional + 56)?;
    let header_size = read32(bytes, optional + 60)?;
    let entry_rva = read32(bytes, optional + 16)?;
    if image_size > MAX_IMAGE_BYTES {
        return Err(LoadError::Capacity);
    }
    if image_base == 0
        || !image_base.is_multiple_of(65536)
        || image_size == 0
        || !image_size.is_multiple_of(PAGE_SIZE)
        || u64::from(image_base) + u64::from(image_size) > 1 << 32
        || header_size == 0
        || !header_size.is_multiple_of(FILE_ALIGNMENT)
        || (header_size as usize) < table_end
        || header_size as usize > bytes.len()
    {
        return Err(LoadError::Malformed);
    }
    let header_end = page_extent(header_size);
    if header_end > u64::from(image_size) {
        return Err(LoadError::Malformed);
    }
    let header_pages = (header_end / u64::from(PAGE_SIZE)) as u32;
    let mut plan = ImagePlan {
        metadata: ImageMetadata32 {
            image_base,
            image_size,
            entry_point: 0,
            mapped_pages: header_pages,
        },
        header_size: header_size as usize,
        header_pages,
        sections: [Section::EMPTY; SECTION_LIMIT],
        section_count,
        relocation_directory,
        import_directory,
        iat_directory,
    };
    let mut previous_virtual_end = header_end;
    let mut previous_raw_end = u64::from(header_size);
    let mut entry_found = false;
    for index in 0..section_count {
        let offset = table + index * 40;
        let virtual_size = read32(bytes, offset + 8)?;
        let rva = read32(bytes, offset + 12)?;
        let raw_size = read32(bytes, offset + 16)?;
        let raw_pointer = read32(bytes, offset + 20)?;
        let flags = read32(bytes, offset + 36)?;
        if flags & !0xe000_00e0 != 0
            || bytes[offset + 24..offset + 36]
                .iter()
                .any(|byte| *byte != 0)
        {
            return Err(LoadError::Unsupported);
        }
        let span = page_extent(virtual_size.max(raw_size));
        let end = u64::from(rva) + span;
        let raw_end = u64::from(raw_pointer) + u64::from(raw_size);
        if virtual_size == 0
            || !rva.is_multiple_of(PAGE_SIZE)
            || u64::from(rva) < previous_virtual_end
            || end > u64::from(image_size)
            || (raw_size == 0 && raw_pointer != 0)
            || (raw_size != 0
                && (!raw_size.is_multiple_of(FILE_ALIGNMENT)
                    || !raw_pointer.is_multiple_of(FILE_ALIGNMENT)
                    || u64::from(raw_pointer) < previous_raw_end
                    || raw_end > bytes.len() as u64))
        {
            return Err(LoadError::Malformed);
        }
        let bits = ((flags >> 30) & 1) | ((flags >> 30) & 2) | ((flags >> 27) & 4);
        let permissions = Permissions::from_bits(bits as u8).ok_or(LoadError::Malformed)?;
        let pages = (span / u64::from(PAGE_SIZE)) as u32;
        plan.sections[index] = Section {
            rva,
            pages,
            raw_pointer: raw_pointer as usize,
            raw_size: raw_size as usize,
            virtual_size,
            permissions,
        };
        plan.metadata.mapped_pages += pages;
        if flags & 0x2000_0000 != 0
            && entry_rva >= rva
            && u64::from(entry_rva) < u64::from(rva) + u64::from(virtual_size.min(raw_size))
        {
            entry_found = true;
        }
        previous_virtual_end = end;
        if raw_size != 0 {
            previous_raw_end = raw_end;
        }
    }
    if !entry_found {
        return Err(LoadError::Malformed);
    }
    plan.metadata.entry_point = image_base + entry_rva;
    Ok(plan)
}

fn page_extent(size: u32) -> u64 {
    u64::from(size).div_ceil(u64::from(PAGE_SIZE)) * u64::from(PAGE_SIZE)
}

fn read16(bytes: &[u8], offset: usize) -> Result<u16, LoadError> {
    let bytes = bytes.get(offset..offset + 2).ok_or(LoadError::Malformed)?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read32(bytes: &[u8], offset: usize) -> Result<u32, LoadError> {
    let bytes = bytes.get(offset..offset + 4).ok_or(LoadError::Malformed)?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}
