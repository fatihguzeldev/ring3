use super::{DispatchError, GuestMemory, MemoryError, guest};

const MAX_BASES: u32 = 64;
const MAX_TYPE_NAME: u32 = 256;

pub(super) fn dynamic_cast(memory: &GuestMemory, args: &[u32]) -> Result<u32, DispatchError> {
    let [object, vf_delta, source_type, target_type, is_reference] =
        <[u32; 5]>::try_from(args).expect("crt call arity");
    if object == 0 {
        return Ok(0);
    }
    if vf_delta != 0 || source_type == 0 || target_type == 0 || is_reference > 1 {
        return Err(DispatchError::Unsupported);
    }

    let vtable = word(memory, object)?;
    let locator_address = vtable.checked_sub(4).ok_or(DispatchError::Unsupported)?;
    let locator = word(memory, locator_address)?;
    let mut locator_words = [0; 5];
    guest::read_words(memory, locator, &mut locator_words)?;
    let [signature, object_offset, construction_offset, _, hierarchy] = locator_words;
    if signature != 0 || construction_offset != 0 {
        return Err(DispatchError::Unsupported);
    }
    let complete = object
        .checked_sub(object_offset)
        .ok_or(DispatchError::Unsupported)?;

    let mut hierarchy_words = [0; 4];
    guest::read_words(memory, hierarchy, &mut hierarchy_words)?;
    let [signature, attributes, count, bases] = hierarchy_words;
    if signature != 0 || attributes & !1 != 0 || !(1..=MAX_BASES).contains(&count) {
        return Err(DispatchError::Unsupported);
    }

    let source_name = type_name(memory, source_type)?;
    let target_name = type_name(memory, target_type)?;
    let mut source_matches = 0;
    let mut target = None;
    let mut ambiguous_target = false;
    for index in 0..count {
        let entry = bases
            .checked_add(index * 4)
            .ok_or(DispatchError::Unsupported)?;
        let descriptor_address = word(memory, entry)?;
        let mut descriptor = [0; 6];
        guest::read_words(memory, descriptor_address, &mut descriptor)?;
        let [type_address, _, offset, virtual_base, _, base_attributes] = descriptor;
        if virtual_base != u32::MAX || base_attributes & !0x40 != 0 {
            return Err(DispatchError::Unsupported);
        }
        let name = type_name(memory, type_address)?;
        if name == source_name && offset == object_offset {
            source_matches += 1;
        }
        if name == target_name && target.replace(offset).is_some() {
            ambiguous_target = true;
        }
    }
    if source_matches != 1 || ambiguous_target {
        return Err(DispatchError::Unsupported);
    }
    match target {
        Some(offset) => complete
            .checked_add(offset)
            .ok_or(DispatchError::Unsupported),
        None if is_reference == 0 => Ok(0),
        None => Err(DispatchError::Unsupported),
    }
}

fn word(memory: &GuestMemory, address: u32) -> Result<u32, MemoryError> {
    let mut output = [0];
    guest::read_words(memory, address, &mut output)?;
    Ok(output[0])
}

fn type_name(memory: &GuestMemory, descriptor: u32) -> Result<Vec<u8>, DispatchError> {
    let start = descriptor
        .checked_add(8)
        .ok_or(MemoryError::AddressOverflow)?;
    let mut name = Vec::new();
    for offset in 0..MAX_TYPE_NAME {
        let address = start
            .checked_add(offset)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut byte = [0];
        memory.read(u64::from(address), &mut byte)?;
        if byte[0] == 0 {
            return if name.is_empty() {
                Err(DispatchError::Unsupported)
            } else {
                Ok(name)
            };
        }
        name.push(byte[0]);
    }
    Err(DispatchError::Unsupported)
}
