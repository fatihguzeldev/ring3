mod tables;
pub(super) fn encode(id: u16, character: char) -> Option<[u8; 2]> {
    let value = tables::lookup(id, u16::try_from(character as u32).ok()?)?;
    Some([(value >> 8) as u8, value as u8])
}
