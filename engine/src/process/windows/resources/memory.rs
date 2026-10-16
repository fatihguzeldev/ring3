use super::ResourceError32 as E;
use crate::{
    loader::ResourceIdentifier32 as Identifier,
    memory::{Access, AddressSpace, GuestAddress},
    windows::text::CodePage32,
};
pub(super) fn identifier(
    memory: &AddressSpace,
    pointer: u32,
    wide: bool,
    code_page: CodePage32,
) -> Result<Identifier, E> {
    if pointer <= u32::from(u16::MAX) {
        return Ok(Identifier::Id(pointer));
    }
    let mut units = Vec::new();
    let mut ansi = Vec::new();
    for index in 0..=32767u32 {
        let at = pointer
            .checked_add(index * if wide { 2 } else { 1 })
            .ok_or(E::InvalidParameter)?;
        let value = if wide {
            let mut bytes = [0; 2];
            memory.read(GuestAddress(at), &mut bytes)?;
            u16::from_le_bytes(bytes)
        } else {
            let mut bytes = [0];
            memory.read(GuestAddress(at), &mut bytes)?;
            u16::from(bytes[0])
        };
        if value == 0 {
            if !wide {
                units = code_page
                    .decode(&ansi, false)
                    .map_err(|_| E::InvalidParameter)?;
            }
            if units.first() == Some(&35) {
                let mut id = 0u32;
                if units.len() == 1 {
                    return Err(E::InvalidParameter);
                }
                for &unit in &units[1..] {
                    if !(48..=57).contains(&unit) {
                        return Err(E::InvalidParameter);
                    }
                    id = id
                        .checked_mul(10)
                        .and_then(|value| value.checked_add(u32::from(unit - 48)))
                        .ok_or(E::InvalidParameter)?;
                }
                return Ok(Identifier::Id(id));
            }
            return Ok(Identifier::Name(units));
        }
        if index == 32767 {
            return Err(E::InvalidParameter);
        }
        if wide {
            units.try_reserve(1).map_err(|_| E::Allocation)?;
            units.push(value);
        } else {
            ansi.try_reserve(1).map_err(|_| E::Allocation)?;
            ansi.push(value as u8);
        }
    }
    unreachable!()
}
pub(super) fn output(memory: &AddressSpace, pointer: u32, length: usize) -> Result<(), E> {
    if pointer == 0 {
        return Err(E::InvalidParameter);
    }
    memory.check_span(GuestAddress(pointer), length, Access::Write)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{PageRange, Permissions};

    #[test]
    fn resource_names_use_configured_dbcs_and_preserve_integer_identifiers() {
        let mut memory = AddressSpace::new(2).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x10000), 1).unwrap(),
                Permissions::READ_WRITE,
            )
            .unwrap();
        memory
            .write(
                GuestAddress(0x10000),
                &[0x83, 0x65, 0x83, 0x58, 0x83, 0x67, 0],
            )
            .unwrap();
        let page = CodePage32::from_id(932).unwrap();
        assert_eq!(
            identifier(&memory, 0x10000, false, page).unwrap(),
            Identifier::Name("テスト".encode_utf16().collect())
        );
        assert_eq!(
            identifier(&memory, 123, false, page).unwrap(),
            Identifier::Id(123)
        );
        memory.write(GuestAddress(0x10000), b"#456\0").unwrap();
        assert_eq!(
            identifier(&memory, 0x10000, false, page).unwrap(),
            Identifier::Id(456)
        );
    }
}
