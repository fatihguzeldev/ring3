use super::AutomationError32 as E;
pub(crate) fn bstr_bytes(payload: &[u8], maximum: usize) -> Result<Vec<u8>, E> {
    let length = payload
        .len()
        .checked_add(6)
        .filter(|&n| n <= maximum && n <= u32::MAX as usize)
        .ok_or(E::Capacity)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).map_err(|_| E::Allocation)?;
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(payload);
    bytes.resize(length, 0);
    Ok(bytes)
}
