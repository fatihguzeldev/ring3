use super::MessageError32 as E;

pub(crate) struct MessageTableEntry32<'a> {
    pub bytes: &'a [u8],
    pub wide: bool,
}
fn word(bytes: &[u8], at: usize) -> Result<u32, E> {
    Ok(u32::from_le_bytes(
        bytes
            .get(at..at.checked_add(4).ok_or(E::InvalidParameter)?)
            .ok_or(E::InvalidParameter)?
            .try_into()
            .unwrap(),
    ))
}
pub(crate) fn message_table_entry(
    bytes: &[u8],
    id: u32,
) -> Result<Option<MessageTableEntry32<'_>>, E> {
    let count = word(bytes, 0)? as usize;
    let header = count
        .checked_mul(12)
        .and_then(|n| n.checked_add(4))
        .filter(|&n| n <= bytes.len())
        .ok_or(E::InvalidParameter)?;
    for block in 0..count {
        let begin = word(bytes, 4 + block * 12)?;
        let end = word(bytes, 8 + block * 12)?;
        let mut at = word(bytes, 12 + block * 12)? as usize;
        if begin > end
            || at < header
            || u64::from(end) - u64::from(begin) + 1 > (bytes.len() / 4) as u64
        {
            return Err(E::InvalidParameter);
        }
        if !(begin..=end).contains(&id) {
            continue;
        }
        for message in begin..=id {
            let record = bytes.get(at..).ok_or(E::InvalidParameter)?;
            let length = u16::from_le_bytes(
                record
                    .get(..2)
                    .ok_or(E::InvalidParameter)?
                    .try_into()
                    .unwrap(),
            ) as usize;
            let flags = u16::from_le_bytes(
                record
                    .get(2..4)
                    .ok_or(E::InvalidParameter)?
                    .try_into()
                    .unwrap(),
            );
            if length < 4 || flags > 1 {
                return Err(E::InvalidParameter);
            }
            let text = record.get(4..length).ok_or(E::InvalidParameter)?;
            let width = if flags == 1 { 2 } else { 1 };
            let terminator = text
                .chunks_exact(width)
                .position(|unit| unit.iter().all(|&b| b == 0))
                .ok_or(E::InvalidParameter)?;
            if message == id {
                return Ok(Some(MessageTableEntry32 {
                    bytes: &text[..terminator * width],
                    wide: flags == 1,
                }));
            }
            at = at.checked_add(length).ok_or(E::InvalidParameter)?;
        }
    }
    Ok(None)
}
